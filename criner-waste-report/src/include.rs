use crate::{Patterns, TarHeader, result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

fn build_include_matcher(patterns: &[String]) -> Gitignore {
    let mut builder = GitignoreBuilder::new("");
    for pattern in patterns {
        builder.add_line(None, pattern).expect("valid include patterns");
    }
    builder.build().expect("valid include patterns")
}

fn includes(matcher: &Gitignore, path: &str) -> bool {
    matcher.matched_path_or_any_parents(path, false).is_ignore()
}

pub(crate) struct ResolvedInclude {
    pub(crate) patterns: Patterns,
    pub(crate) waste: Vec<TarHeader>,
}

pub(crate) fn resolve(include: Patterns, crate_entries: Vec<TarHeader>) -> ResolvedInclude {
    let matcher = build_include_matcher(&include);
    let decoded_paths: Vec<&str> = crate_entries
        .iter()
        .map(|entry| result::tar_path_to_utf8_str(&entry.path))
        .collect();
    let included_before_reorder: Vec<bool> = decoded_paths.iter().map(|path| includes(&matcher, path)).collect();

    // A multi-file pattern whose matches were all negated was likely negated on purpose
    let matches_exactly_one_excluded_file = |pattern: &str| {
        pattern.strip_prefix('!').is_none() && {
            let glob = result::make_glob(pattern).compile_matcher();
            let mut matched = decoded_paths
                .iter()
                .zip(&included_before_reorder)
                .filter(|(path, _)| glob.is_match(path));
            matches!((matched.next(), matched.next()), (Some((_, false)), None))
        }
    };
    // for conflicts, last pattern wins, so negations should be last
    let (kept, moved_last): (Vec<_>, Vec<_>) = include
        .into_iter()
        .partition(|pattern| !matches_exactly_one_excluded_file(pattern));
    let patterns: Patterns = kept.into_iter().chain(moved_last).collect();

    let matcher = build_include_matcher(&patterns);
    let waste = crate_entries
        .into_iter()
        .filter(|entry| {
            let path = result::tar_path_to_utf8_str(&entry.path);
            !cargo_always_packages(path) && !includes(&matcher, path)
        })
        .collect();

    ResolvedInclude { patterns, waste }
}

fn cargo_always_packages(path: &str) -> bool {
    matches!(
        path,
        "Cargo.toml" | "Cargo.toml.orig" | "Cargo.lock" | ".cargo_vcs_info.json"
    )
}
