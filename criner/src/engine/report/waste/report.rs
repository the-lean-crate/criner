use crate::{
    model::{TarHeader, TaskResult},
    Result,
};
use async_trait::async_trait;
use serde_derive::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(PartialEq, Eq, Debug)]
pub enum GlobKind {
    Include,
    #[allow(dead_code)]
    Exclude,
}

#[derive(PartialEq, Eq, Debug)]
pub enum Severity {
    #[allow(dead_code)]
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

fn tar_path_to_path_no_strip(bytes: &[u8]) -> &Path {
    &Path::new(std::str::from_utf8(bytes).expect("valid utf8 paths in crate archive"))
}

// NOTE: Actually there only seem to be files in these archives, but let's be safe
// There are definitely no directories
fn entry_is_file(entry_type: u8) -> bool {
    tar::EntryType::new(entry_type).is_file()
}

fn split_to_matched_and_unmatched<'entries>(
    entries: Vec<TarHeader>,
    globset: &globset::GlobSet,
) -> (Vec<TarHeader>, Vec<TarHeader>) {
    let mut unmatched = Vec::new();
    let matched = entries
        .into_iter()
        .filter_map(|e| {
            if globset.is_match(tar_path_to_utf8_str(&e.path)) {
                Some(e)
            } else {
                unmatched.push(e);
                None
            }
        })
        .collect();
    (matched, unmatched)
}

fn directories_of(entries: &[TarHeader]) -> Vec<TarHeader> {
    let mut directories = BTreeSet::new();
    for e in entries {
        if entry_is_file(e.entry_type) {
            if let Some(parent) = tar_path_to_path_no_strip(&e.path).parent() {
                directories.insert(parent);
            }
        }
    }
    directories
        .into_iter()
        .map(|k| TarHeader {
            path: k.to_str().expect("utf8 paths").as_bytes().to_owned(),
            size: 0,
            entry_type: tar::EntryType::Directory.as_byte(),
        })
        .collect()
}

fn globset_from(patterns: impl IntoIterator<Item = impl AsRef<str>>) -> Result<globset::GlobSet> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns.into_iter() {
        builder.add(globset::Glob::new(pattern.as_ref())?);
    }
    builder.build().map_err(Into::into)
}

fn split_by_matching_directories(
    entries: Vec<TarHeader>,
    directories: &[TarHeader],
) -> Vec<TarHeader> {
    // Shortcut: we assume '/' as path separator, which is true for all paths in crates.io except for 214 :D - it's OK to not find things in that case.
    let globs = globset_from(directories.iter().map(|e| {
        let mut s = tar_path_to_utf8_str(&e.path).to_string();
        s.push_str("/**");
        s
    }))
    .expect("always valid globs from directories");
    split_to_matched_and_unmatched(entries, &globs).0
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
    fn compute_includes(
        entries: Vec<TarHeader>,
        _include_patterns: Vec<String>,
        exclude_patterns: Vec<String>,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let fix = Fix {
            kind: GlobKind::Include,
            description: (
                Severity::Warn,
                "Excludes are ignored if includes are given.".to_string(),
            ),
            globs: vec![],
        };
        let exclude_globs =
            globset_from(&exclude_patterns).expect("only valid exclude globs in Cargo.toml");
        let directories = directories_of(&entries);

        let (mut entries_that_should_be_excluded, remaining_entries) =
            split_to_matched_and_unmatched(entries, &exclude_globs);
        let (directories_that_should_be_excluded, _remaining_directories) =
            split_to_matched_and_unmatched(directories, &exclude_globs);
        let entries_that_should_be_excluded_by_directory =
            split_by_matching_directories(remaining_entries, &directories_that_should_be_excluded);
        entries_that_should_be_excluded
            .extend(entries_that_should_be_excluded_by_directory.into_iter());

        (Some(fix), entries_that_should_be_excluded)
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
                    (Some(_includes), None) => unimplemented!(
                        "allow everything, assuming they know what they are doing, but flag tests"
                    ),
                    (None, Some(_excludes)) => unimplemented!("check for accidental includes"),
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
    use super::{Fix, GlobKind, Report, Severity};
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
                wasted_bytes: 813680,
                wasted_files: 23,
                suggested_fix: Some(Fix {
                    kind: GlobKind::Include,
                    description: (Severity::Warn, "Excludes are ignored if includes are given".into()),
                    globs: vec![]
                })
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
