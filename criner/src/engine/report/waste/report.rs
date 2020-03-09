use crate::{
    model::{TarHeader, TaskResult},
    Result,
};
use async_trait::async_trait;
use serde_derive::Deserialize;
use std::path::{Path, PathBuf};

#[derive(PartialEq, Eq, Debug)]
pub enum GlobKind {
    Include,
    Exclude,
}

#[derive(PartialEq, Eq, Debug)]
pub enum Severity {
    Info,
    Warn,
}

#[derive(PartialEq, Eq, Debug)]
pub struct Fix {
    kind: GlobKind,
    description: (Severity, String),
    globs: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}
#[derive(Deserialize)]
struct CargoConfig {
    package: Package,
}

#[derive(PartialEq, Eq, Debug)]
pub enum Report {
    None,
    Version {
        total_size_in_bytes: u64,
        total_files: u64,
        wasted_bytes: u64,
        wasted_files: u64,
        suggested_fix: Option<Fix>,
    },
}

fn tar_path_to_utf8_str(mut bytes: &[u8]) -> &str {
    // Tar paths include the parent directory, cut it to crate relative paths
    if let Some(pos) = bytes.iter().position(|b| *b == b'/' || *b == b'\\') {
        bytes = bytes.get(pos + 1..).unwrap_or(bytes);
    }
    std::str::from_utf8(bytes).expect("valid utf8 paths in crate archive")
}

fn tar_path_to_path(bytes: &[u8]) -> &Path {
    &Path::new(tar_path_to_utf8_str(bytes))
}

fn is_tar_file(entry_type: u8) -> bool {
    tar::EntryType::new(entry_type).is_file()
}

impl Report {
    fn package_from_entries(entries: &[(TarHeader, Vec<u8>)]) -> Package {
        entries
            .iter()
            .find_map(|(h, v)| {
                if tar_path_to_path(&h.path).ends_with("Cargo.toml") {
                    Some(
                        toml::from_slice::<CargoConfig>(&v)
                            .expect("valid Cargo.toml format")
                            .package,
                    )
                } else {
                    None
                }
            })
            .expect("Cargo.toml to always be present in the exploded crate")
    }

    fn counts_from(entries: Vec<TarHeader>) -> (u64, u64) {
        (
            entries.len() as u64,
            entries.iter().map(|e| e.size).sum::<u64>(),
        )
    }

    fn globset_from(patterns: &[String]) -> Result<globset::GlobSet> {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in patterns {
            builder.add(globset::Glob::new(pattern)?);
        }
        builder.build().map_err(Into::into)
    }

    fn compute_includes(
        entries: Vec<TarHeader>,
        include_patterns: Vec<String>,
        exclude_patterns: Vec<String>,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        dbg!(&exclude_patterns);
        let include_patterns =
            Self::globset_from(&include_patterns).expect("only valid include globs in Cargo.toml");
        let exclude_patterns =
            Self::globset_from(&exclude_patterns).expect("only valid exclude globs in Cargo.toml");
        let included_files = Report::apply_globset_to_tarfiles(&entries, &include_patterns);
        let files_that_should_be_excluded =
            Report::apply_globset_to_utf8_str(&included_files, &exclude_patterns);
        dbg!(&included_files);
        dbg!(files_that_should_be_excluded);
        unimplemented!()
    }

    fn apply_globset_to_utf8_str<'files>(
        files: &'files [&str],
        globset: &globset::GlobSet,
    ) -> Vec<&'files str> {
        files
            .iter()
            .cloned()
            .filter(|p| globset.is_match(p))
            .collect()
    }
    fn apply_globset_to_tarfiles<'entries>(
        entries: &'entries [TarHeader],
        globset: &globset::GlobSet,
    ) -> Vec<&'entries str> {
        entries
            .iter()
            .filter_map(|e| {
                if is_tar_file(e.entry_type) && globset.is_match(tar_path_to_utf8_str(&e.path)) {
                    Some(tar_path_to_utf8_str(&e.path))
                } else {
                    None
                }
            })
            .collect()
    }
}

impl From<TaskResult> for Report {
    fn from(result: TaskResult) -> Report {
        match result {
            TaskResult::ExplodedCrate {
                entries_meta_data,
                selected_entries,
            } => {
                let total_size_in_bytes = entries_meta_data.iter().map(|e| e.size).sum();
                let total_files = entries_meta_data.len() as u64;
                let package = Self::package_from_entries(&selected_entries);
                let (suggested_fix, wasted_files) = match (package.include, package.exclude) {
                    (Some(includes), Some(excludes)) => {
                        Self::compute_includes(entries_meta_data, includes, excludes)
                    }
                    (Some(includes), None) => unimplemented!(
                        "allow everything, assuming they know what they are doing, but flag tests"
                    ),
                    (None, Some(excludes)) => unimplemented!("check for accidental includes"),
                    (None, None) => unimplemented!("flag everything that isn't standard includes"),
                };
                let (wasted_files, wasted_bytes) = Self::counts_from(wasted_files);
                Report::Version {
                    total_size_in_bytes,
                    total_files,
                    wasted_bytes,
                    wasted_files,
                    suggested_fix,
                }
            }
            _ => unreachable!("need caller to assure we get exploded crates only"),
        }
    }
}

#[async_trait]
impl crate::engine::report::generic::Aggregate for Report {
    fn merge(self, other: Self) -> Self {
        other
    }

    async fn complete_all(self, _out_dir: PathBuf, _progress: prodash::tree::Item) -> Result<()> {
        Ok(())
    }
    async fn complete_crate(
        &mut self,
        _out_dir: &Path,
        _crate_name: &str,
        _progress: &mut prodash::tree::Item,
    ) -> Result<()> {
        Ok(())
    }
}

impl Default for Report {
    fn default() -> Self {
        Report::None
    }
}

#[cfg(test)]
mod from_extract_crate {
    use super::Report;
    use crate::model::TaskResult;

    const GNIR: &[u8] =
        include_bytes!("../../../../tests/fixtures/gnir-0.14.0-alpha3-extract_crate-1.0.0.bin");
    const SOVRIN: &[u8] = include_bytes!(
        "../../../../tests/fixtures/sovrin-client.0.1.0-179-extract_crate-1.0.0.bin"
    );
    const MOZJS: &[u8] =
        include_bytes!("../../../../tests/fixtures/mozjs_sys-0.67.1-extract_crate-1.0.0.bin");

    #[test]
    fn gnir() {
        assert_eq!(
            Report::from(TaskResult::from(GNIR)),
            Report::Version {
                total_size_in_bytes: 15216510,
                total_files: 382,
                wasted_bytes: 0,
                wasted_files: 0,
                suggested_fix: None
            },
            "correct size and assume people are aware if includes are present, but excludes must be expressed as includes as they are mutually exclusive"
        );
    }
    #[test]
    fn sovrin_client() {
        assert_eq!(
            Report::from(TaskResult::from(SOVRIN)),
            Report::Version {
                total_size_in_bytes: 20283032,
                total_files: 479,
                wasted_files: 0,
                wasted_bytes: 0,
                suggested_fix: None
            },
            "build.rs is used but there are a bunch of extra directories that can be ignored and are not needed by the build, no manual includes/excludes"
        );
    }
    #[test]
    fn mozjs() {
        // todo: filter tests, benches, examples, image file formats, docs, allow everything in src/ , but be aware of tests/specs
        assert_eq!(
            Report::from(TaskResult::from(MOZJS)),
            Report::Version {
                total_size_in_bytes: 161225785,
                total_files: 13187,
                wasted_files: 0,
                wasted_bytes: 0,
                suggested_fix: None
            },
            "build.rs + excludes in Cargo.toml - this leaves a chance for accidental includes for which we provide an updated exclude list"
        );
    }
}
