mod html;
mod merge;
mod result;

use crate::model::TarHeader;
use serde_derive::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, path::PathBuf};

pub use result::{globset_from_patterns, tar_path_to_utf8_str};

pub type Patterns = Vec<String>;

pub struct TarPackage {
    /// Meta data of all entries in the crate
    pub entries_meta_data: Vec<TarHeader>,
    /// The actual content of selected files, Cargo.*, build.rs and lib/main
    /// IMPORTANT: This file may be partial and limited in size unless it is Cargo.toml, which
    /// is always complete.
    /// Note that these are also present in entries_meta_data.
    pub entries: Vec<(TarHeader, Vec<u8>)>,
}

#[derive(PartialEq, Eq, Debug, Clone, Deserialize, Serialize)]
pub struct PotentialWaste {
    pub patterns_to_fix: Patterns,
    pub potential_waste: Vec<TarHeader>,
}

#[derive(PartialEq, Eq, Debug, Clone, Deserialize, Serialize)]
pub enum Fix {
    ImprovedInclude {
        include: Patterns,
        include_removed: Patterns,
        potential: Option<PotentialWaste>,
        has_build_script: bool,
    },
    EnrichedExclude {
        exclude: Patterns,
        exclude_added: Patterns,
        has_build_script: bool,
    },
    NewInclude {
        include: Patterns,
        has_build_script: bool,
    },
    RemoveExcludeAndUseInclude {
        include_added: Patterns,
        include: Patterns,
        include_removed: Patterns,
    },
    RemoveExclude,
}

impl Fix {
    pub fn merge(
        self,
        rhs: Option<PotentialWaste>,
        mut waste: Vec<TarHeader>,
    ) -> (Fix, Vec<TarHeader>) {
        match (self, rhs) {
            (
                Fix::NewInclude {
                    mut include,
                    has_build_script,
                },
                Some(potential),
            ) => (
                Fix::NewInclude {
                    has_build_script,
                    include: {
                        include.extend(potential.patterns_to_fix);
                        include
                    },
                },
                {
                    waste.extend(potential.potential_waste);
                    waste
                },
            ),
            (lhs, _) => (lhs, waste),
        }
    }
}

#[derive(Default, Deserialize)]
pub struct CargoConfig {
    pub package: Option<PackageSection>,
    pub lib: Option<SectionWithPath>,
    pub bin: Option<Vec<SectionWithPath>>,
}

impl CargoConfig {
    pub fn actual_or_expected_build_script_path(&self) -> &str {
        self.build_script_path().unwrap_or("build.rs")
    }
    pub fn build_script_path(&self) -> Option<&str> {
        self.package.as_ref().and_then(|p| p.build_script_path())
    }
    pub fn lib_path(&self) -> &str {
        self.lib
            .as_ref()
            .and_then(|l| l.path.as_deref())
            .unwrap_or("src/lib.rs")
    }
    pub fn bin_paths(&self) -> Vec<&str> {
        self.bin
            .as_ref()
            .map(|l| l.iter().filter_map(|s| s.path.as_deref()).collect())
            .unwrap_or_else(|| vec!["src/main.rs"])
    }
}

impl From<&[u8]> for CargoConfig {
    fn from(v: &[u8]) -> Self {
        toml::from_slice::<CargoConfig>(v).unwrap_or_default() // you would think all of them parse OK, but that's wrong :D
    }
}

#[derive(Default, Deserialize)]
pub struct SectionWithPath {
    pub path: Option<String>,
}

#[derive(Default, Deserialize)]
pub struct PackageSection {
    pub include: Option<Patterns>,
    pub exclude: Option<Patterns>,
    pub build: Option<toml::value::Value>,
}

impl PackageSection {
    pub fn build_script_path(&self) -> Option<&str> {
        self.build.as_ref().and_then(|s| s.as_str())
    }
}

pub type WastedFile = (String, u64);

#[derive(Default, Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct AggregateFileInfo {
    pub total_bytes: u64,
    pub total_files: u64,
}

#[derive(Default, Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct VersionInfo {
    pub all: AggregateFileInfo,
    pub waste: AggregateFileInfo,
    pub waste_latest_version: Option<(String, AggregateFileInfo)>,
    pub potential_gains: Option<AggregateFileInfo>,
}

pub type AggregateVersionInfo = VersionInfo;

pub type Dict<T> = BTreeMap<String, T>;

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub enum Report {
    Version {
        crate_name: String,
        crate_version: String,
        total_size_in_bytes: u64,
        total_files: u64,
        wasted_files: Vec<WastedFile>,
        suggested_fix: Option<Fix>,
    },
    Crate {
        crate_name: String,
        total_size_in_bytes: u64,
        total_files: u64,
        info_by_version: Dict<VersionInfo>,
        wasted_by_extension: Dict<AggregateFileInfo>,
    },
    CrateCollection {
        total_size_in_bytes: u64,
        total_files: u64,
        info_by_crate: Dict<AggregateVersionInfo>,
        wasted_by_extension: Dict<AggregateFileInfo>,
    },
}

fn remove_implicit_entries(entries: &mut Vec<TarHeader>) {
    entries.retain(|e| {
        let p = tar_path_to_utf8_str(&e.path);
        p != ".cargo_vcs_info.json" && p != "Cargo.toml.orig"
    });
}

const TOP_LEVEL_REPORT_NAME: &str = "__top-level-report__";

fn path_from_prefix(out_dir: &Path, prefix: &str) -> PathBuf {
    use crate::engine::report::generic::Generator;
    out_dir.join(format!(
        "{}-{}-{}.rmp",
        prefix,
        super::Generator::name(),
        super::Generator::version()
    ))
}

impl Report {
    fn path_to_storage_location(&self, out_dir: &Path) -> PathBuf {
        use Report::*;
        let prefix = match self {
            Version { crate_name, .. } | Crate { crate_name, .. } => crate_name.as_str(),
            CrateCollection { .. } => TOP_LEVEL_REPORT_NAME,
        };
        path_from_prefix(out_dir, prefix)
    }

    pub fn from_package(
        crate_name: &str,
        crate_version: &str,
        TarPackage {
            mut entries_meta_data,
            entries,
        }: TarPackage,
    ) -> Report {
        remove_implicit_entries(&mut entries_meta_data);
        let total_size_in_bytes = entries_meta_data.iter().map(|e| e.size).sum();
        let total_files = entries_meta_data.len() as u64;
        let cargo_config = Self::cargo_config_from_entries(&entries);
        let (includes, excludes, compile_time_includes, build_script_name) =
            Self::cargo_config_into_includes_excludes(cargo_config, &entries, &entries_meta_data);
        let (suggested_fix, wasted_files) =
            match (includes, excludes, build_script_name, compile_time_includes) {
                (Some(includes), Some(excludes), _presence_of_build_script_not_relevant, _) => {
                    Self::compute_includes_from_includes_and_excludes(
                        entries_meta_data,
                        includes,
                        excludes,
                    )
                }
                (Some(includes), None, build_script_name, _) => {
                    Self::enrich_includes(entries_meta_data, includes, build_script_name.is_some())
                }
                (None, Some(excludes), build_script_name, compile_time_includes) => {
                    Self::enrich_excludes(
                        entries_meta_data,
                        excludes,
                        compile_time_includes,
                        build_script_name.is_some(),
                    )
                }
                (None, None, build_script_name, compile_time_includes) => Self::standard_includes(
                    entries_meta_data,
                    build_script_name,
                    compile_time_includes,
                ),
            };
        let wasted_files = Self::convert_to_wasted_files(wasted_files);
        Report::Version {
            crate_name: crate_name.into(),
            crate_version: crate_version.into(),
            total_size_in_bytes,
            total_files,
            wasted_files,
            suggested_fix,
        }
    }
}

pub mod criner;
