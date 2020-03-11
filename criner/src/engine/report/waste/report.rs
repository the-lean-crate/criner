use crate::{
    model::{TarHeader, TaskResult},
    Result,
};
use async_trait::async_trait;
use serde_derive::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub type Patterns = Vec<String>;

#[derive(PartialEq, Eq, Debug)]
pub enum Fix {
    EnrichedInclude {
        include: Patterns,
        include_added: Patterns,
        include_removed: Patterns,
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
struct Package {
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    build: Option<String>,
}

#[derive(Default, Deserialize)]
struct CargoConfig {
    package: Option<Package>,
}

type WastedFile = (String, u64);

#[derive(PartialEq, Eq, Debug)]
pub enum Report {
    None,
    Version {
        total_size_in_bytes: u64,
        total_files: u64,
        wasted_files: Vec<WastedFile>,
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

fn standard_exclude_patterns() -> &'static [&'static str] {
    &[
        "**/*.jpg",
        "**/*.jpeg",
        "**/*.jpeg",
        "**/*.png",
        "**/*.gif",
        "**/*.bmp",
        "**/doc/*",
        "**/docs/*",
        "**/benches/*",
        "**/benchmark/*",
        "**/benchmarks/*",
        "**/test/*",
        "**/tests/*",
        "**/testing/*",
        "**/spec/*",
        "**/specs/*",
        "**/*_test.*",
        "**/*_tests.*",
        "**/*_spec.*",
        "**/*_specs.*",
        "**/example/*",
        "**/examples/*",
        "**/target/*",
        "**/build/*",
        "**/out/*",
        "**/tmp/*",
        "**/lib/*",
        "**/etc/*",
        "**/testdata/*",
        "**/samples/*",
        "**/assets/*",
        "**/maps/*",
        "**/media/*",
        "**/fixtures/*",
        "**/node_modules/*",
    ]
}
fn standard_include_patterns() -> &'static [&'static str] {
    &[
        "src/**/*",
        "Cargo.*",
        "license.*",
        "LICENSE.*",
        "license",
        "LICENSE",
        "readme.*",
        "README.*",
        "readme",
        "README",
        "changelog.*",
        "CHANGELOG.*",
        "changelog",
        "CHANGELOG",
    ]
}

fn globset_from(patterns: impl IntoIterator<Item = impl AsRef<str>>) -> globset::GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns.into_iter() {
        builder.add(make_glob(pattern.as_ref()));
    }
    builder
        .build()
        .expect("multiple globs to always fit into a globset")
}

fn split_by_matching_directories(
    entries: Vec<TarHeader>,
    directories: &[TarHeader],
) -> (Vec<TarHeader>, Vec<TarHeader>) {
    // Shortcut: we assume '/' as path separator, which is true for all paths in crates.io except for 214 :D - it's OK to not find things in that case.
    let globs = globset_from(directories.iter().map(|e| {
        let mut s = tar_path_to_utf8_str(&e.path).to_string();
        s.push_str("/**");
        s
    }));
    split_to_matched_and_unmatched(entries, &globs)
}

fn filter_implicit_includes(
    include_patterns: &mut Patterns,
    mut removed_include_patterns: impl AsMut<Patterns>,
) {
    let removed_include_patterns = removed_include_patterns.as_mut();
    let mut current_removed_count = removed_include_patterns.len();
    loop {
        if let Some(pos_to_remove) = include_patterns.iter().position(|p| {
            p == "Cargo.toml" || p == "Cargo.lock" || p == "./Cargo.toml" || p == "./Cargo.lock"
        }) {
            removed_include_patterns.push(include_patterns[pos_to_remove].to_owned());
            include_patterns.remove(pos_to_remove);
        }
        if current_removed_count != removed_include_patterns.len() {
            current_removed_count = removed_include_patterns.len();
            continue;
        }
        break;
    }
}

fn find_include_patterns_that_incorporate_exclude_patterns(
    entries_to_exclude: &[TarHeader],
    entries_to_include: &[TarHeader],
    include_patterns: Patterns,
) -> (Patterns, Patterns, Patterns) {
    let mut added_include_patterns = Vec::new();
    let mut removed_include_patterns = Vec::new();
    let mut new_include_patterns = Vec::with_capacity(include_patterns.len());
    for pattern in include_patterns {
        let glob = make_glob(&pattern);
        let matcher = glob.compile_matcher();
        if entries_to_exclude
            .iter()
            .any(|e| matcher.is_match(tar_path_to_path(&e.path)))
        {
            removed_include_patterns.push(pattern);
            let added_includes: Vec<_> = entries_to_include
                .iter()
                .filter(|e| matcher.is_match(tar_path_to_path(&e.path)))
                .map(|e| tar_path_to_utf8_str(&e.path).to_string())
                .collect();
            added_include_patterns.extend(added_includes.clone().into_iter());
            new_include_patterns.extend(added_includes.into_iter());
        } else {
            new_include_patterns.push(pattern);
        }
    }

    filter_implicit_includes(&mut new_include_patterns, &mut removed_include_patterns);
    (
        new_include_patterns,
        added_include_patterns,
        removed_include_patterns,
    )
}

fn make_glob(pattern: &str) -> globset::Glob {
    globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .case_insensitive(false)
        .backslash_escape(true) // most paths in crates.io are forward slashes, there are only 214 or so with backslashes
        .build()
        .expect("valid include patterns")
}

fn simplify_standard_includes(
    includes: &'static [&'static str],
    entries: &[TarHeader],
) -> Patterns {
    let is_recursive_glob = |p: &str| p.contains("**");
    let mut out_patterns: Vec<_> = includes
        .iter()
        .filter(|p| is_recursive_glob(*p))
        .map(|p| p.to_string())
        .collect();
    for pattern in includes.iter().filter(|p| !is_recursive_glob(p)) {
        let matcher = globset::Glob::new(pattern)
            .expect("valid pattern")
            .compile_matcher();
        out_patterns.extend(
            entries
                .iter()
                .filter(|e| matcher.is_match(tar_path_to_utf8_str(&e.path)))
                .map(|e| tar_path_to_utf8_str(&e.path).to_owned()),
        );
    }
    filter_implicit_includes(&mut out_patterns, Vec::new());
    out_patterns
}

fn find_in_entries<'buffer>(
    entries_with_buffer: &'buffer [(TarHeader, Vec<u8>)],
    entries: &[TarHeader],
    name: &str,
) -> Option<(TarHeader, Option<&'buffer [u8]>)> {
    entries_with_buffer
        .iter()
        .find_map(|(h, v)| {
            if tar_path_to_path(&h.path).ends_with(name) {
                Some((h.clone(), Some(v.as_slice())))
            } else {
                None
            }
        })
        .or_else(|| {
            entries.iter().find_map(|e| {
                if tar_path_to_path(&e.path).ends_with(name) {
                    Some((e.clone(), None))
                } else {
                    None
                }
            })
        })
}

fn matches_in_set_a_but_not_in_set_b(
    mut initial_a: Patterns,
    set_a: &[&'static str],
    set_b: &globset::GlobSet,
    mut entries: Vec<TarHeader>,
) -> (Vec<TarHeader>, Patterns, Patterns) {
    let set_a_len = initial_a.len();
    for exclude_pattern in set_a {
        let exclude_glob = make_glob(exclude_pattern).compile_matcher();
        if entries
            .iter()
            .any(|e| exclude_glob.is_match(tar_path_to_utf8_str(&e.path)))
        {
            if entries
                .iter()
                .any(|e| set_b.is_match(tar_path_to_utf8_str(&e.path)))
            {
                entries.retain(|e| !exclude_glob.is_match(tar_path_to_utf8_str(&e.path)));
                if entries.is_empty() {
                    break;
                }
            } else {
                initial_a.push(exclude_pattern.to_string());
            }
        }
    }

    let new_excludes = initial_a
        .get(set_a_len..)
        .map(|v| v.to_vec())
        .unwrap_or_else(Vec::new);
    (entries, initial_a, new_excludes)
}

fn simplify_standard_excludes_and_match_against_standard_includes(
    potential_waste: Vec<TarHeader>,
    existing_exclude: Patterns,
) -> (Vec<TarHeader>, Patterns, Patterns) {
    let include_globs = globset_from(standard_include_patterns());
    matches_in_set_a_but_not_in_set_b(
        existing_exclude,
        standard_exclude_patterns(),
        &include_globs,
        potential_waste,
    )
}

impl Report {
    fn package_from_entries(entries: &[(TarHeader, Vec<u8>)]) -> Package {
        find_in_entries(entries, &[], "Cargo.toml")
            .and_then(|(_e, v)| {
                v.and_then(|v| {
                    toml::from_slice::<CargoConfig>(&v)
                        .unwrap_or_default() // some Cargo.toml files have build: true, which doesn't parse for us. TODO: maybe parse manually
                        .package
                })
            })
            .unwrap_or_default()
    }

    fn convert_to_wasted_files(entries: Vec<TarHeader>) -> Vec<WastedFile> {
        entries
            .into_iter()
            .map(|e| (tar_path_to_utf8_str(&e.path).to_owned(), e.size))
            .collect()
    }

    fn standard_includes(
        entries: Vec<TarHeader>,
        entries_with_buffer: Vec<(TarHeader, Vec<u8>)>,
        buildscript_name: Option<String>,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let include_patterns = standard_include_patterns();
        let maybe_build_script = find_in_entries(
            &entries_with_buffer,
            &entries,
            &buildscript_name.unwrap_or_else(|| "build.rs".to_owned()),
        )
        .map(|(entry, _buf)| tar_path_to_utf8_str(&entry.path).to_owned());
        let include_globs = globset_from(include_patterns);
        let (included_entries, excluded_entries) =
            split_to_matched_and_unmatched(entries, &include_globs);

        let mut include_patterns = simplify_standard_includes(include_patterns, &included_entries);
        let has_build_script = match maybe_build_script {
            Some(build_script_name) => {
                include_patterns.push(build_script_name);
                true
            }
            None => false,
        };

        (
            Some(Fix::NewInclude {
                include: include_patterns,
                has_build_script,
            }),
            excluded_entries,
        )
    }

    fn compute_includes_from_includes_and_excludes(
        entries: Vec<TarHeader>,
        include: Patterns,
        exclude: Patterns,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let exclude_globs = globset_from(&exclude);
        let directories = directories_of(&entries);

        let (mut entries_that_should_be_excluded, remaining_entries) =
            split_to_matched_and_unmatched(entries, &exclude_globs);
        let (directories_that_should_be_excluded, _remaining_directories) =
            split_to_matched_and_unmatched(directories, &exclude_globs);
        let (entries_that_should_be_excluded_by_directory, remaining_entries) =
            split_by_matching_directories(remaining_entries, &directories_that_should_be_excluded);
        entries_that_should_be_excluded
            .extend(entries_that_should_be_excluded_by_directory.into_iter());

        let fix = if entries_that_should_be_excluded.is_empty() {
            Fix::RemoveExclude
        } else {
            let (include, include_added, include_removed) =
                find_include_patterns_that_incorporate_exclude_patterns(
                    &entries_that_should_be_excluded,
                    &remaining_entries,
                    include,
                );
            Fix::RemoveExcludeAndUseInclude {
                include_added,
                include,
                include_removed,
            }
        };

        (Some(fix), entries_that_should_be_excluded)
    }

    fn enrich_includes(
        entries: Vec<TarHeader>,
        entries_with_buffer: Vec<(TarHeader, Vec<u8>)>,
        mut include: Patterns,
        buildscript_name: Option<String>,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let mut include_removed = Vec::new();
        let has_build_script = find_in_entries(
            &entries_with_buffer,
            &entries,
            &buildscript_name.unwrap_or_else(|| "Cargo.toml".into()),
        )
        .is_some();
        filter_implicit_includes(&mut include, &mut include_removed);
        let include_globs = globset_from(&include);
        let (unmatched_files, include, include_added) = matches_in_set_a_but_not_in_set_b(
            include,
            standard_include_patterns(),
            &include_globs,
            entries,
        );

        let include_globs = globset_from(&include);
        let (_remaining_files, wasted_files) =
            split_to_matched_and_unmatched(unmatched_files, &include_globs);
        if wasted_files.is_empty() {
            (None, Vec::new())
        } else {
            (
                Some(Fix::EnrichedInclude {
                    include,
                    include_removed,
                    include_added,
                    has_build_script,
                }),
                wasted_files,
            )
        }
    }

    fn enrich_excludes(
        entries: Vec<TarHeader>,
        entries_with_buffer: Vec<(TarHeader, Vec<u8>)>,
        exclude: Patterns,
        buildscript_name: Option<String>,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let has_build_script = find_in_entries(
            &entries_with_buffer,
            &entries,
            &buildscript_name.unwrap_or_else(|| "build.rs".into()),
        )
        .is_some();
        let standard_excludes = standard_exclude_patterns();
        let exclude_globs = globset_from(standard_excludes);
        let (potential_waste, _remaining) = split_to_matched_and_unmatched(entries, &exclude_globs);
        let (wasted_files, exclude, exclude_added) =
            simplify_standard_excludes_and_match_against_standard_includes(
                potential_waste,
                exclude,
            );
        if wasted_files.is_empty() {
            (None, Vec::new())
        } else {
            (
                Some(Fix::EnrichedExclude {
                    exclude,
                    exclude_added,
                    has_build_script,
                }),
                wasted_files,
            )
        }
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
                let (suggested_fix, wasted_files) =
                    match (package.include, package.exclude, package.build) {
                        (Some(includes), Some(excludes), _build_script_does_not_matter) => {
                            Self::compute_includes_from_includes_and_excludes(
                                entries_meta_data,
                                includes,
                                excludes,
                            )
                        }
                        (Some(includes), None, build) => Self::enrich_includes(
                            entries_meta_data,
                            selected_entries,
                            includes,
                            build,
                        ),
                        (None, Some(excludes), build) => Self::enrich_excludes(
                            entries_meta_data,
                            selected_entries,
                            excludes,
                            build,
                        ),
                        (None, None, build) => {
                            Self::standard_includes(entries_meta_data, selected_entries, build)
                        }
                    };
                let wasted_files = Self::convert_to_wasted_files(wasted_files);
                Report::Version {
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
