use super::{CargoConfig, Fix, Patterns, PotentialWaste, Report, TarHeader, WastedFile};
use std::{collections::BTreeSet, path::Path, path::PathBuf};

lazy_static! {
    static ref COMPILE_TIME_INCLUDE: regex::bytes::Regex =
        regex::bytes::Regex::new(r##"include_(str|bytes)!\("(?P<include>.+?)"\)"##)
            .expect("valid statically known regex");
    static ref BUILD_SCRIPT_PATHS: regex::bytes::Regex =
        regex::bytes::Regex::new(r##""cargo:rerun-if-changed=(?P<path>.+?)"|"(?P<path_like>.+?)""##)
            .expect("valid statically known regex");
    static ref STANDARD_EXCLUDES_GLOBSET: globset::GlobSet = globset_from_patterns(standard_exclude_patterns());
    static ref STANDARD_EXCLUDE_MATCHERS: Vec<(&'static str, globset::GlobMatcher)> = standard_exclude_patterns()
        .iter()
        .cloned()
        .map(|p| (p, make_glob(p).compile_matcher()))
        .collect();
    static ref STANDARD_INCLUDE_GLOBS: Vec<globset::Glob> =
        standard_include_patterns().iter().map(|p| make_glob(p)).collect();
    static ref STANDARD_INCLUDE_MATCHERS: Vec<(&'static str, globset::GlobMatcher)> = standard_include_patterns()
        .iter()
        .cloned()
        .map(|p| (p, make_glob(p).compile_matcher()))
        .collect();
}

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
    entry_type == b'\x00' || entry_type == b'0'
}

fn split_to_matched_and_unmatched(
    entries: Vec<TarHeader>,
    globset: &globset::GlobSet,
) -> (Vec<TarHeader>, Vec<TarHeader>) {
    let mut unmatched = Vec::new();
    #[allow(clippy::unnecessary_filter_map)] // we need to keep the unmatched element
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
    let tar_directory_entry = b'5';
    directories
        .into_iter()
        .map(|k| TarHeader {
            path: k.to_str().expect("utf8 paths").as_bytes().to_owned(),
            size: 0,
            entry_type: tar_directory_entry,
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
        "**/doc/**/*",
        "**/docs/**/*",
        "**/benches/**/*",
        "**/benchmark/**/*",
        "**/benchmarks/**/*",
        "**/test/**/*",
        "**/*_test.*",
        "**/*_test/**/*",
        "**/tests/**/*",
        "**/*_tests.*",
        "**/*_tests/**/*",
        "**/testing/**/*",
        "**/spec/**/*",
        "**/*_spec.*",
        "**/*_spec/**/*",
        "**/specs/**/*",
        "**/*_specs.*",
        "**/*_specs/**/*",
        "**/example/**/*",
        "**/examples/**/*",
        "**/target/**/*",
        "**/build/**/*",
        "**/out/**/*",
        "**/tmp/**/*",
        "**/etc/**/*",
        "**/testdata/**/*",
        "**/samples/**/*",
        "**/assets/**/*",
        "**/maps/**/*",
        "**/media/**/*",
        "**/fixtures/**/*",
        "**/node_modules/**/*",
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

pub fn globset_from_patterns(patterns: impl IntoIterator<Item = impl AsRef<str>>) -> globset::GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns.into_iter() {
        builder.add(make_glob(pattern.as_ref()));
    }
    builder.build().expect("multiple globs to always fit into a globset")
}

pub fn globset_from_globs_and_patterns(
    globs: &[globset::Glob],
    patterns: impl IntoIterator<Item = impl AsRef<str>>,
) -> globset::GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    for glob in globs.iter() {
        builder.add(glob.clone());
    }
    for pattern in patterns.into_iter() {
        builder.add(make_glob(pattern.as_ref()));
    }
    builder.build().expect("multiple globs to always fit into a globset")
}

fn split_by_matching_directories(
    entries: Vec<TarHeader>,
    directories: &[TarHeader],
) -> (Vec<TarHeader>, Vec<TarHeader>) {
    // Shortcut: we assume '/' as path separator, which is true for all paths in crates.io except for 214 :D - it's OK to not find things in that case.
    let globs = globset_from_patterns(directories.iter().map(|e| {
        let mut s = tar_path_to_utf8_str(&e.path).to_string();
        s.push_str("/**");
        s
    }));
    split_to_matched_and_unmatched(entries, &globs)
}

fn remove_implicit_includes(include_patterns: &mut Patterns, mut removed_include_patterns: impl AsMut<Patterns>) {
    let removed_include_patterns = removed_include_patterns.as_mut();
    let mut current_removed_count = removed_include_patterns.len();
    loop {
        if let Some(pos_to_remove) = include_patterns.iter().position(|p| {
            p == "Cargo.toml.orig"
                || p == "Cargo.toml"
                || p == "Cargo.lock"
                || p == "./Cargo.toml"
                || p == "./Cargo.lock"
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

/// These input patterns are **file paths**, and we would like to make them into the smallest possible amount of **include patterns**.
/// These patterns must not accidentally match the 'pattern_to_not_match', as it is the pattern they are supposed to replace.
/// Note that one could also use negated patterns, so keep the 'pattern to not match', but add a specific negation.
/// HELP WANTED: This could be done by finding the common ancestors and resolve to patterns that match their children most specifically
/// See https://github.com/the-lean-crate/criner/issues/2
fn turn_file_paths_into_patterns(added_include_patterns: Patterns, _pattern_to_not_match: &str) -> Patterns {
    added_include_patterns
}

fn find_include_patterns_that_incorporate_exclude_patterns(
    entries_to_exclude: &[TarHeader],
    entries_to_include: &[TarHeader],
    include_patterns: Patterns,
) -> (Patterns, Patterns, Patterns) {
    let mut added_include_patterns = Vec::new();
    let mut removed_include_patterns = Vec::new();
    let mut all_include_patterns = Vec::with_capacity(include_patterns.len());
    for pattern in include_patterns {
        let glob = make_glob(&pattern);
        let include = glob.compile_matcher();
        if entries_to_exclude
            .iter()
            .any(|e| include.is_match(tar_path_to_path(&e.path)))
        {
            let added_includes = turn_file_paths_into_patterns(
                entries_to_include
                    .iter()
                    .filter(|e| include.is_match(tar_path_to_path(&e.path)))
                    .map(|e| tar_path_to_utf8_str(&e.path).to_string())
                    .collect(),
                &pattern,
            );
            removed_include_patterns.push(pattern);
            added_include_patterns.extend(added_includes.clone().into_iter());
            all_include_patterns.extend(added_includes.into_iter());
        } else {
            all_include_patterns.push(pattern);
        }
    }

    remove_implicit_includes(&mut all_include_patterns, &mut removed_include_patterns);
    (all_include_patterns, added_include_patterns, removed_include_patterns)
}

fn make_glob(pattern: &str) -> globset::Glob {
    globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .case_insensitive(false)
        .backslash_escape(true) // most paths in crates.io are forward slashes, there are only 214 or so with backslashes
        .build()
        .expect("valid include patterns")
}

fn simplify_includes<'a>(
    include_patterns_and_matchers: impl Iterator<Item = &'a (&'a str, globset::GlobMatcher)>,
    mut entries: Vec<TarHeader>,
) -> Patterns {
    let mut out_patterns = Vec::new();
    let mut matched = Vec::<String>::new();
    for (pattern, glob) in include_patterns_and_matchers {
        matched.clear();
        matched.extend(
            entries
                .iter()
                .filter(|e| glob.is_match(tar_path_to_utf8_str(&e.path)))
                .map(|e| tar_path_to_utf8_str(&e.path).to_string()),
        );
        match matched.len() {
            0 => {}
            1 => {
                out_patterns.push(matched[0].clone());
            }
            _ => {
                out_patterns.push((*pattern).to_string());
            }
        }
        entries.retain(|e| !matched.iter().any(|p| p == tar_path_to_utf8_str(&e.path)));
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
            if tar_path_to_utf8_str(&h.path) == name {
                Some((h.clone(), Some(v.as_slice())))
            } else {
                None
            }
        })
        .or_else(|| {
            entries.iter().find_map(|e| {
                if tar_path_to_utf8_str(&e.path) == name {
                    Some((e.clone(), None))
                } else {
                    None
                }
            })
        })
}

fn matches_in_set_a_but_not_in_set_b(
    mut patterns_to_amend: Patterns,
    set_a: &[(&str, globset::GlobMatcher)],
    set_b: &globset::GlobSet,
    mut entries: Vec<TarHeader>,
) -> (Vec<TarHeader>, Patterns, Patterns) {
    let set_a_len = patterns_to_amend.len();
    let all_entries = entries.clone();
    for (pattern_a, glob_a) in set_a {
        if entries.iter().any(|e| glob_a.is_match(tar_path_to_utf8_str(&e.path))) {
            entries.retain(|e| !glob_a.is_match(tar_path_to_utf8_str(&e.path)));
            if entries.is_empty() {
                break;
            }
            if set_b.is_empty() {
                patterns_to_amend.push((*pattern_a).to_string());
                continue;
            }

            if entries.iter().any(|e| set_b.is_match(tar_path_to_utf8_str(&e.path))) {
                patterns_to_amend.push((*pattern_a).to_string());
            }
        }
    }

    let patterns_in_set_a_which_do_not_match_a_pattern_in_set_b = patterns_to_amend
        .get(set_a_len..)
        .map(|v| v.to_vec())
        .unwrap_or_else(Vec::new);
    let (entries, _) = split_to_matched_and_unmatched(
        all_entries,
        &globset_from_patterns(&patterns_in_set_a_which_do_not_match_a_pattern_in_set_b),
    );
    (
        entries,
        patterns_to_amend,
        patterns_in_set_a_which_do_not_match_a_pattern_in_set_b,
    )
}

/// Takes something like "src/deep/lib.rs" and "../data/foo.bin" and turns it into "src/data/foo.bin", replicating
/// the way include_str/bytes interprets include paths. Thus it makes these paths relative to the crate, instead of
/// relative to the source file they are included in.
fn to_crate_relative_path(source_file_path: impl AsRef<Path>, relative_path: impl AsRef<Path>) -> String {
    use std::path::Component::*;
    let relative_path = relative_path.as_ref();
    let source_path = source_file_path
        .as_ref()
        .parent()
        .expect("directory containing the file");
    let leading_parent_path_components = relative_path
        .components()
        .take_while(|c| matches!(c, ParentDir | CurDir))
        .filter(|c| matches!(c, ParentDir))
        .count();
    let components_to_take_from_relative_path = relative_path
        .components()
        .filter(|c| !matches!(c, CurDir))
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
    let include_iter =
        globset_from_globs_and_patterns(&STANDARD_INCLUDE_GLOBS, compile_time_include.iter().map(|s| s.as_str()));
    matches_in_set_a_but_not_in_set_b(
        existing_exclude,
        &STANDARD_EXCLUDE_MATCHERS,
        &include_iter,
        potential_waste,
    )
}

fn included_paths_of(file: Option<(TarHeader, Option<&[u8]>)>) -> Vec<String> {
    file.and_then(|(header, maybe_data)| maybe_data.map(|d| (header, d)))
        .map(|(header, data)| {
            COMPILE_TIME_INCLUDE
                .captures_iter(data)
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

/// HELP WANTED find the largest common ancestors (e.g. curl/* for curl/foo/* and curl/bar/*) and return these
/// instead of the ones they contain. This can help speeding up later use of the patterns, as there are less of them.
fn optimize_directories(dir_patterns: Vec<String>) -> Vec<String> {
    dir_patterns
}

fn find_paths_mentioned_in_build_script(build: Option<(TarHeader, Option<&[u8]>)>) -> Vec<String> {
    build
        .and_then(|(header, maybe_data)| maybe_data.map(|d| (header, d)))
        .map(|(_, data)| {
            let mut v: Vec<_> = BUILD_SCRIPT_PATHS
                .captures_iter(data)
                .map(|cap| {
                    std::str::from_utf8(
                        cap.name("path")
                            .or_else(|| cap.name("path_like"))
                            .expect("one of the two matches")
                            .as_bytes(),
                    )
                    .expect("valid utf8")
                    .to_string()
                })
                .filter(|p| {
                    !(p.contains('{')
                        || p.contains(' ')
                        || p.contains('@')
                        || (!p.as_bytes().iter().any(|b| b.is_ascii_digit()) && &p.to_uppercase() == p) // probably environment variable
                        || p.starts_with("cargo:")
                        || p.starts_with('-'))
                })
                .collect();
            let dirs: BTreeSet<_> = v
                .iter()
                .filter_map(|p| {
                    Path::new(p)
                        .parent()
                        .filter(|p| p.components().count() > 0)
                        .and_then(|p| p.to_str().map(|s| s.to_string()))
                })
                .collect();
            let possible_patterns = if dirs.is_empty() {
                v.extend(v.clone().into_iter().map(|p| format!("{}/*", p)));
                v
            } else {
                let mut dirs = optimize_directories(dirs.into_iter().map(|d| format!("{}/*", d)).collect());
                dirs.extend(v.into_iter().map(|p| format!("{}/*", p)));
                dirs
            };
            possible_patterns
                .into_iter()
                .filter_map(|p| globset::Glob::new(&p).ok().map(|_| p))
                .collect()
        })
        .unwrap_or_default()
}

fn potential_negated_includes(entries: Vec<TarHeader>, patters_to_avoid: globset::GlobSet) -> Option<PotentialWaste> {
    let (entries_we_would_remove, patterns, _) =
        matches_in_set_a_but_not_in_set_b(Vec::new(), &STANDARD_EXCLUDE_MATCHERS, &patters_to_avoid, entries);
    let negated_patterns: Vec<_> = patterns.into_iter().map(|s| format!("!{}", s)).collect();
    if negated_patterns.is_empty() {
        None
    } else {
        Some(PotentialWaste {
            patterns_to_fix: negated_patterns,
            potential_waste: entries_we_would_remove,
        })
    }
}

fn add_to_includes_if_non_default(file_path: &str, include: &mut Patterns) {
    let recursive_pattern = Path::new(file_path).parent().expect("file path as input").join("**");
    if !standard_include_patterns().contains(&recursive_pattern.join("*").to_str().expect("utf8 only")) {
        include.push(recursive_pattern.join("*.rs").to_str().expect("utf 8 only").to_string())
    }
}

fn non_greedy_patterns<S: AsRef<str>>(patterns: impl IntoIterator<Item = S>) -> impl Iterator<Item = S> {
    patterns
        .into_iter()
        .filter(|p| !p.as_ref().starts_with('*') && p.as_ref().ends_with('*'))
}

impl Report {
    pub(crate) fn cargo_config_from_entries(entries: &[(TarHeader, Vec<u8>)]) -> CargoConfig {
        find_in_entries(entries, &[], "Cargo.toml")
            .and_then(|(_e, v)| v.map(CargoConfig::from))
            .unwrap_or_default()
    }

    pub(crate) fn convert_to_wasted_files(entries: Vec<TarHeader>) -> Vec<WastedFile> {
        entries
            .into_iter()
            .map(|e| (tar_path_to_utf8_str(&e.path).to_owned(), e.size))
            .collect()
    }

    pub(crate) fn standard_includes(
        entries: Vec<TarHeader>,
        build_script_name: Option<String>,
        compile_time_include: Option<Patterns>,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let mut compile_time_include = compile_time_include.unwrap_or_default();
        let has_build_script = match build_script_name {
            Some(build_script_name) => {
                compile_time_include.push(build_script_name);
                true
            }
            None => false,
        };
        let include_globs =
            globset_from_globs_and_patterns(&STANDARD_INCLUDE_GLOBS, compile_time_include.iter().map(|s| s.as_str()));
        let (included_entries, excluded_entries) = split_to_matched_and_unmatched(entries, &include_globs);

        let compile_time_include_matchers: Vec<_> = compile_time_include
            .iter()
            .map(|s| (s.as_str(), make_glob(s).compile_matcher()))
            .collect();
        let include_patterns = simplify_includes(
            STANDARD_INCLUDE_MATCHERS
                .iter()
                .chain(compile_time_include_matchers.iter()),
            included_entries.clone(),
        );
        let potential = potential_negated_includes(
            included_entries,
            globset_from_patterns(non_greedy_patterns(&compile_time_include)),
        );

        if excluded_entries.is_empty() && potential.is_none() {
            (None, Vec::new())
        } else {
            let (fix, waste) = Fix::NewInclude {
                include: include_patterns,
                has_build_script,
            }
            .merge(potential, excluded_entries);
            (Some(fix), waste)
        }
    }

    pub(crate) fn compute_includes_from_includes_and_excludes(
        entries: Vec<TarHeader>,
        include: Patterns,
        exclude: Patterns,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let exclude_globs = globset_from_patterns(&exclude);
        let directories = directories_of(&entries);

        let (mut entries_that_should_be_excluded, remaining_entries) =
            split_to_matched_and_unmatched(entries, &exclude_globs);
        let (directories_that_should_be_excluded, _remaining_directories) =
            split_to_matched_and_unmatched(directories, &exclude_globs);
        let (entries_that_should_be_excluded_by_directory, remaining_entries) =
            split_by_matching_directories(remaining_entries, &directories_that_should_be_excluded);
        entries_that_should_be_excluded.extend(entries_that_should_be_excluded_by_directory.into_iter());

        let fix = if entries_that_should_be_excluded.is_empty() {
            Some(Fix::RemoveExclude)
        } else {
            let (include, include_added, include_removed) = find_include_patterns_that_incorporate_exclude_patterns(
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

    /// This implementation respects all explicitly given includes but proposes potential excludes based on our exclude list.
    pub(crate) fn enrich_includes(
        entries: Vec<TarHeader>,
        mut include: Patterns,
        has_build_script: bool,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let mut include_removed = Vec::new();
        remove_implicit_includes(&mut include, &mut include_removed);

        (
            if include_removed.is_empty() {
                None
            } else {
                let potential =
                    potential_negated_includes(entries, globset_from_patterns(non_greedy_patterns(&include)));
                Some(Fix::ImprovedInclude {
                    include,
                    include_removed,
                    has_build_script,
                    potential,
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
        let (potential_waste, _remaining) = split_to_matched_and_unmatched(entries, &STANDARD_EXCLUDES_GLOBSET);
        let (wasted_files, exclude, exclude_added) = simplify_standard_excludes_and_match_against_standard_includes(
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

    pub(crate) fn cargo_config_into_includes_excludes(
        config: CargoConfig,
        entries_with_buffer: &[(TarHeader, Vec<u8>)],
        entries: &[TarHeader],
    ) -> (Option<Patterns>, Option<Patterns>, Option<Patterns>, Option<String>) {
        let mut maybe_build_script_path = config.build_script_path().map(|s| s.to_owned());
        let compile_time_includes = {
            let mut includes_parsed_from_files = Vec::new();
            includes_parsed_from_files.extend(included_paths_of(find_in_entries(
                &entries_with_buffer,
                &entries,
                config.lib_path(),
            )));
            add_to_includes_if_non_default(config.lib_path(), &mut includes_parsed_from_files);
            for path in config.bin_paths() {
                includes_parsed_from_files.extend(included_paths_of(find_in_entries(
                    &entries_with_buffer,
                    &entries,
                    path,
                )));
                add_to_includes_if_non_default(path, &mut includes_parsed_from_files);
            }

            let build_script_name = config.actual_or_expected_build_script_path();
            let maybe_data = find_in_entries(&entries_with_buffer, &entries, build_script_name);
            maybe_build_script_path =
                maybe_build_script_path.or_else(|| maybe_data.as_ref().map(|_| build_script_name.to_owned()));
            includes_parsed_from_files.extend(find_paths_mentioned_in_build_script(maybe_data));

            if includes_parsed_from_files.is_empty() {
                None
            } else {
                Some(includes_parsed_from_files)
            }
        };

        let package = config.package.unwrap_or_default();
        (
            package.include,
            package.exclude,
            compile_time_includes,
            maybe_build_script_path,
        )
    }
}
