mod html;
mod merge;
mod result;

use crate::{
    model::{TarHeader, TaskResult},
    Result,
};
use async_trait::async_trait;
use serde_derive::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, path::PathBuf};

pub use result::{globset_from, tar_path_to_utf8_str};

pub type Patterns = Vec<String>;

#[derive(PartialEq, Eq, Debug, Clone, Deserialize, Serialize)]
pub struct PotentialWaste {
    pub patterns_to_fix: Patterns,
    pub potential_waste: Vec<WastedFile>,
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

#[derive(Default, Deserialize)]
pub struct Package {
    include: Option<Patterns>,
    exclude: Option<Patterns>,
    build: Option<String>,
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

#[async_trait]
impl crate::engine::report::generic::Aggregate for Report {
    fn merge(self, other: Self) -> Self {
        use Report::*;
        match (self, other) {
            (lhs @ Version { .. }, rhs @ Version { .. }) => {
                merge::crate_from_version(lhs).merge(rhs)
            }
            (version @ Version { .. }, krate @ Crate { .. }) => krate.merge(version),
            (version @ Version { .. }, collection @ CrateCollection { .. }) => {
                collection.merge(version)
            }
            (collection @ CrateCollection { .. }, version @ Version { .. }) => {
                collection.merge(merge::crate_from_version(version))
            }
            (krate @ Crate { .. }, collection @ CrateCollection { .. }) => collection.merge(krate),
            (
                Crate {
                    crate_name: lhs_crate_name,
                    total_size_in_bytes: lhs_tsb,
                    total_files: lhs_tf,
                    info_by_version,
                    wasted_by_extension,
                },
                Version {
                    crate_name: rhs_crate_name,
                    crate_version,
                    total_size_in_bytes: rhs_tsb,
                    total_files: rhs_tf,
                    wasted_files,
                    suggested_fix,
                },
            ) => {
                if lhs_crate_name == rhs_crate_name {
                    Crate {
                        crate_name: lhs_crate_name,
                        total_size_in_bytes: lhs_tsb + rhs_tsb,
                        total_files: lhs_tf + rhs_tf,
                        info_by_version: merge::map_into_map(
                            info_by_version,
                            merge::version_to_new_version_map(
                                crate_version,
                                rhs_tsb,
                                rhs_tf,
                                &wasted_files,
                            ),
                        ),
                        wasted_by_extension: merge::vec_into_map_by_extension(
                            wasted_by_extension,
                            wasted_files,
                        ),
                    }
                } else {
                    merge::collection_from_crate(
                        lhs_crate_name,
                        lhs_tsb,
                        lhs_tf,
                        info_by_version,
                        wasted_by_extension,
                    )
                    .merge(Version {
                        crate_name: rhs_crate_name,
                        crate_version,
                        total_size_in_bytes: rhs_tsb,
                        total_files: rhs_tf,
                        wasted_files,
                        suggested_fix,
                    })
                }
            }
            (
                Crate {
                    crate_name: lhs_crate_name,
                    total_size_in_bytes: lhs_tsb,
                    total_files: lhs_tf,
                    info_by_version: lhs_ibv,
                    wasted_by_extension: lhs_wbe,
                },
                Crate {
                    crate_name: rhs_crate_name,
                    total_size_in_bytes: rhs_tsb,
                    total_files: rhs_tf,
                    info_by_version: rhs_ibv,
                    wasted_by_extension: rhs_wbe,
                },
            ) => {
                if lhs_crate_name != rhs_crate_name {
                    merge::collection_from_crate(lhs_crate_name, lhs_tsb, lhs_tf, lhs_ibv, lhs_wbe)
                        .merge(Crate {
                            crate_name: rhs_crate_name,
                            total_size_in_bytes: rhs_tsb,
                            total_files: rhs_tf,
                            info_by_version: rhs_ibv,
                            wasted_by_extension: rhs_wbe,
                        })
                } else {
                    Crate {
                        crate_name: lhs_crate_name,
                        total_size_in_bytes: lhs_tsb + rhs_tsb,
                        total_files: lhs_tf + rhs_tf,
                        info_by_version: merge::map_into_map(lhs_ibv, rhs_ibv),
                        wasted_by_extension: merge::map_into_map(lhs_wbe, rhs_wbe),
                    }
                }
            }
            (
                CrateCollection {
                    total_size_in_bytes: lhs_tsb,
                    total_files: lhs_tf,
                    info_by_crate: lhs_ibc,
                    wasted_by_extension: lhs_wbe,
                },
                CrateCollection {
                    total_size_in_bytes: rhs_tsb,
                    total_files: rhs_tf,
                    info_by_crate: rhs_ibc,
                    wasted_by_extension: rhs_wbe,
                },
            ) => CrateCollection {
                total_size_in_bytes: lhs_tsb + rhs_tsb,
                total_files: lhs_tf + rhs_tf,
                info_by_crate: merge::map_into_map(lhs_ibc, rhs_ibc),
                wasted_by_extension: merge::map_into_map(lhs_wbe, rhs_wbe),
            },
            (
                CrateCollection {
                    total_size_in_bytes: lhs_tsb,
                    total_files: lhs_tf,
                    info_by_crate,
                    wasted_by_extension: lhs_wbe,
                },
                Crate {
                    crate_name,
                    total_size_in_bytes: rhs_tsb,
                    total_files: rhs_tf,
                    info_by_version,
                    wasted_by_extension: rhs_wbe,
                },
            ) => CrateCollection {
                total_size_in_bytes: lhs_tsb + rhs_tsb,
                total_files: lhs_tf + rhs_tf,
                wasted_by_extension: merge::map_into_map(lhs_wbe, rhs_wbe),
                info_by_crate: merge::map_into_map(
                    info_by_crate,
                    merge::crate_info_from_version_info(crate_name, info_by_version),
                ),
            },
        }
    }

    async fn complete(&mut self, out_dir: &Path, progress: &mut prodash::tree::Item) -> Result<()> {
        use async_std::prelude::*;
        use horrorshow::Template;

        progress.blocked("writing report to disk", None);
        let report = self.clone();
        let mut buf = String::new();
        report.write_to_string(&mut buf)?;

        async_std::fs::OpenOptions::new()
            .truncate(true)
            .write(true)
            .create(true)
            .open(out_dir.join("index.html"))
            .await?
            .write_all(buf.as_bytes())
            .await
            .map_err(crate::Error::from)
    }
    async fn load_previous_state(
        &self,
        out_dir: &Path,
        progress: &mut prodash::tree::Item,
    ) -> Option<Self> {
        if let Some(path) = self.path_to_storage_location(out_dir) {
            progress.blocked("loading previous waste report from disk", None);
            async_std::fs::read(path)
                .await
                .ok()
                .and_then(|v| rmp_serde::from_read(v.as_slice()).ok())
        } else {
            None
        }
    }
    async fn store_current_state(
        &self,
        out_dir: &Path,
        progress: &mut prodash::tree::Item,
    ) -> Result<()> {
        let path = self
            .path_to_storage_location(out_dir)
            .expect("a path for every occasion");
        progress.blocked("storing current waste report to disk", None);
        let data = rmp_serde::to_vec(self)?;
        async_std::fs::write(path, data).await.map_err(Into::into)
    }
}

impl Report {
    fn path_to_storage_location(&self, out_dir: &Path) -> Option<PathBuf> {
        use crate::engine::report::generic::Generator;
        use Report::*;
        let prefix = match self {
            Version { crate_name, .. } | Crate { crate_name, .. } => crate_name.as_str(),
            CrateCollection { .. } => "__top-level-report__",
        };
        Some(out_dir.join(format!(
            "{}-{}-{}.rmp",
            prefix,
            super::Generator::name(),
            super::Generator::version()
        )))
    }
    pub fn from_result(crate_name: &str, crate_version: &str, result: TaskResult) -> Report {
        match result {
            TaskResult::ExplodedCrate {
                entries_meta_data,
                selected_entries,
            } => {
                let total_size_in_bytes = entries_meta_data.iter().map(|e| e.size).sum();
                let total_files = entries_meta_data.len() as u64;
                let package = Self::package_from_entries(&selected_entries);
                let (includes, excludes, compile_time_includes, build_script_name) =
                    Self::package_into_includes_excludes(
                        package,
                        &selected_entries,
                        &entries_meta_data,
                    );
                let (suggested_fix, wasted_files) =
                    match (includes, excludes, build_script_name, compile_time_includes) {
                        (
                            Some(includes),
                            Some(excludes),
                            _presence_of_build_script_not_relevant,
                            _,
                        ) => Self::compute_includes_from_includes_and_excludes(
                            entries_meta_data,
                            includes,
                            excludes,
                        ),
                        (Some(includes), None, build_script_name, _) => Self::enrich_includes(
                            entries_meta_data,
                            includes,
                            build_script_name.is_some(),
                        ),
                        (None, Some(excludes), build_script_name, compile_time_includes) => {
                            Self::enrich_excludes(
                                entries_meta_data,
                                excludes,
                                compile_time_includes,
                                build_script_name.is_some(),
                            )
                        }
                        (None, None, build_script_name, compile_time_includes) => {
                            Self::standard_includes(
                                entries_meta_data,
                                build_script_name,
                                compile_time_includes,
                            )
                        }
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
            _ => unreachable!("need caller to assure we get exploded crates only"),
        }
    }
}
