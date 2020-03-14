use super::{Fix, Package, Patterns, Report, TarHeader, WastedFile};
use std::path::PathBuf;
use std::{collections::BTreeSet, path::Path};

pub fn tar_path_to_utf8_str(mut bytes: &[u8]) -> &str {
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
        "authors",
        "AUTHORS",
        "license.*",
        "license-*",
        "LICENSE.*",
        "LICENSE-*",
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

pub fn globset_from(patterns: impl IntoIterator<Item = impl AsRef<str>>) -> globset::GlobSet {
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

fn remove_implicit_includes(
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

    remove_implicit_includes(&mut new_include_patterns, &mut removed_include_patterns);
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
    remove_implicit_includes(&mut out_patterns, Vec::new());
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
    mut patterns_to_amend: Patterns,
    set_a: &[&'static str],
    set_b_patterns: impl IntoIterator<Item = impl AsRef<str>>,
    mut entries: Vec<TarHeader>,
) -> (Vec<TarHeader>, Patterns, Patterns) {
    let set_b = globset_from(set_b_patterns);
    let set_a_len = patterns_to_amend.len();
    for pattern_a in set_a {
        let glob_a = make_glob(pattern_a).compile_matcher();
        if entries
            .iter()
            .any(|e| glob_a.is_match(tar_path_to_utf8_str(&e.path)))
        {
            if entries
                .iter()
                .any(|e| set_b.is_match(tar_path_to_utf8_str(&e.path)))
            {
                entries.retain(|e| !glob_a.is_match(tar_path_to_utf8_str(&e.path)));
                if entries.is_empty() {
                    break;
                }
            } else {
                patterns_to_amend.push(pattern_a.to_string());
            }
        }
    }

    let new_excludes = patterns_to_amend
        .get(set_a_len..)
        .map(|v| v.to_vec())
        .unwrap_or_else(Vec::new);
    (entries, patterns_to_amend, new_excludes)
}

/// Takes something like "src/deep/lib.rs" and "../data/foo.bin" and turns it into "src/data/foo.bin", replicating
/// the way include_str/bytes interprets include paths. Thus it makes these paths relative to the crate, instead of
/// relative to the source file they are included in.
fn to_crate_relative_path(
    source_file_path: impl AsRef<Path>,
    relative_path: impl AsRef<Path>,
) -> String {
    use std::path::Component::*;
    let relative_path = relative_path.as_ref();
    let source_path = source_file_path
        .as_ref()
        .parent()
        .expect("directory containing the file");
    let leading_parent_path_components = relative_path
        .components()
        .take_while(|c| match c {
            ParentDir | CurDir => true,
            _ => false,
        })
        .filter(|c| match c {
            ParentDir => true,
            _ => false,
        })
        .count();
    let components_to_take_from_relative_path = relative_path
        .components()
        .filter(|c| match c {
            CurDir => false,
            _ => true,
        })
        .skip(leading_parent_path_components);
    let components_to_take = source_path
        .components()
        .count()
        .saturating_sub(leading_parent_path_components);
    source_path
        .components()
        .take(components_to_take)
        .chain(components_to_take_from_relative_path)
        .fold(PathBuf::new(), |mut p, c| {
            p.push(c);
            p
        })
        .to_str()
        .expect("utf8 only")
        .to_string()
}

fn simplify_standard_excludes_and_match_against_standard_includes(
    potential_waste: Vec<TarHeader>,
    existing_exclude: Patterns,
    compile_time_include: Option<Patterns>,
) -> (Vec<TarHeader>, Patterns, Patterns) {
    let compile_time_include = compile_time_include.unwrap_or_default();
    let include_iter = standard_include_patterns()
        .iter()
        .cloned()
        .chain(compile_time_include.iter().map(|s| s.as_str()));
    matches_in_set_a_but_not_in_set_b(
        existing_exclude,
        standard_exclude_patterns(),
        include_iter,
        potential_waste,
    )
}

fn included_paths_of(file: Option<(TarHeader, Option<&[u8]>)>) -> Vec<String> {
    file.and_then(|(header, maybe_data)| maybe_data.map(|d| (header, d)))
        .map(|(header, data)| {
            let re = regex::bytes::Regex::new(r##"include_(str|bytes)!\("(?P<include>.+?)"\)"##)
                .expect("valid statically known regex");
            re.captures_iter(data)
                .map(|cap| {
                    to_crate_relative_path(
                        tar_path_to_utf8_str(&header.path),
                        std::str::from_utf8(&cap["include"]).expect("utf8 path"),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Report {
    pub(crate) fn package_from_entries(entries: &[(TarHeader, Vec<u8>)]) -> Package {
        use serde_derive::Deserialize;
        #[derive(Default, Deserialize)]
        struct CargoConfig {
            package: Option<Package>,
        }

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

    pub(crate) fn convert_to_wasted_files(entries: Vec<TarHeader>) -> Vec<WastedFile> {
        entries
            .into_iter()
            .filter_map(|e| {
                let path = tar_path_to_utf8_str(&e.path);
                if path != ".cargo_vcs_info.json" {
                    Some((path.to_owned(), e.size))
                } else {
                    None
                }
            })
            .collect()
    }

    pub(crate) fn standard_includes(
        entries: Vec<TarHeader>,
        build_script_name: Option<String>,
        compile_time_include: Option<Patterns>,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let compile_time_include = compile_time_include.unwrap_or_default();
        let include_patterns = standard_include_patterns()
            .iter()
            .cloned()
            .chain(compile_time_include.iter().map(|s| s.as_str()));
        let include_globs = globset_from(include_patterns);
        let (included_entries, excluded_entries) =
            split_to_matched_and_unmatched(entries, &include_globs);

        let mut include_patterns =
            simplify_standard_includes(standard_include_patterns(), &included_entries);
        let has_build_script = match build_script_name {
            Some(build_script_name) => {
                include_patterns.push(build_script_name);
                true
            }
            None => false,
        };
        include_patterns.extend(compile_time_include.into_iter());

        (
            Some(Fix::NewInclude {
                include: include_patterns,
                has_build_script,
            }),
            excluded_entries,
        )
    }

    pub(crate) fn compute_includes_from_includes_and_excludes(
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
            Some(Fix::RemoveExclude)
        } else {
            let (include, include_added, include_removed) =
                find_include_patterns_that_incorporate_exclude_patterns(
                    &entries_that_should_be_excluded,
                    &remaining_entries,
                    include,
                );
            if include_added.is_empty() && include_removed.is_empty() {
                None
            } else {
                Some(Fix::RemoveExcludeAndUseInclude {
                    include_added,
                    include,
                    include_removed,
                })
            }
        };

        (fix, entries_that_should_be_excluded)
    }

    pub(crate) fn enrich_includes(
        _entries: Vec<TarHeader>,
        mut include: Patterns,
        _compile_time_include: Option<Patterns>,
        has_build_script: bool,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let mut include_removed = Vec::new();
        remove_implicit_includes(&mut include, &mut include_removed);

        (
            if include_removed.is_empty() {
                None
            } else {
                Some(Fix::ImprovedInclude {
                    include,
                    include_removed,
                    has_build_script,
                })
            },
            Vec::new(),
        )
    }

    pub(crate) fn enrich_excludes(
        entries: Vec<TarHeader>,
        exclude: Patterns,
        compile_time_include: Option<Patterns>,
        has_build_script: bool,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let standard_excludes = standard_exclude_patterns();
        let exclude_globs = globset_from(standard_excludes);
        let (potential_waste, _remaining) = split_to_matched_and_unmatched(entries, &exclude_globs);
        let (wasted_files, exclude, exclude_added) =
            simplify_standard_excludes_and_match_against_standard_includes(
                potential_waste,
                exclude,
                compile_time_include,
            );
        if wasted_files.is_empty() {
            (None, Vec::new())
        } else {
            (
                if exclude_added.is_empty() {
                    None
                } else {
                    Some(Fix::EnrichedExclude {
                        exclude,
                        exclude_added,
                        has_build_script,
                    })
                },
                wasted_files,
            )
        }
    }

    pub(crate) fn package_into_includes_excludes(
        package: Package,
        entries_with_buffer: &[(TarHeader, Vec<u8>)],
        entries: &[TarHeader],
    ) -> (
        Option<Patterns>,
        Option<Patterns>,
        Option<Patterns>,
        Option<String>,
    ) {
        // TODO: use actual names from package
        let compile_time_includes = {
            let lib_file = find_in_entries(&entries_with_buffer, &entries, "lib.rs");
            let main_file = find_in_entries(&entries_with_buffer, &entries, "main.rs");
            let lib_includes = included_paths_of(lib_file);
            let mut main_includes = included_paths_of(main_file);
            main_includes.extend(lib_includes.into_iter());
            if main_includes.is_empty() {
                None
            } else {
                Some(main_includes)
            }
        };
        let build_script_name = package.build.or_else(|| Some("build.rs".to_owned()));

        (
            package.include,
            package.exclude,
            compile_time_includes,
            build_script_name,
        )
    }
}
