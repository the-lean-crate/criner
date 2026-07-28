use crate::{Fix, PotentialWaste, TarHeader};

fn file(path: &str, size: u64) -> TarHeader {
    TarHeader {
        path: path.as_bytes().to_vec(),
        size,
        entry_type: b'0',
    }
}

fn merged(include: Vec<&str>, patterns_to_fix: Vec<&str>, entries: Vec<TarHeader>) -> (Vec<String>, Vec<TarHeader>) {
    let fix = Fix::NewInclude {
        include: include.into_iter().map(Into::into).collect(),
        has_build_script: false,
    };
    let potential = PotentialWaste {
        patterns_to_fix: patterns_to_fix.into_iter().map(Into::into).collect(),
        potential_waste: Vec::new(),
    };
    match fix.merge(Some(potential), entries) {
        (Fix::NewInclude { include, .. }, waste) => (include, waste),
        _ => unreachable!("merge preserves the NewInclude variant"),
    }
}

#[test]
fn overridden_single_file_pattern_moves_last() {
    let (include, waste) = merged(
        vec!["src/lib.rs", "examples/example.rs"],
        vec!["!**/examples/**/*"],
        vec![file("p-1/src/lib.rs", 10), file("p-1/examples/example.rs", 455)],
    );
    assert_eq!(include, vec!["src/lib.rs", "!**/examples/**/*", "examples/example.rs"]);
    assert_eq!(waste, vec![]);
}

#[test]
fn partially_overridden_patterns_stay() {
    let (include, waste) = merged(
        vec!["src/**/*"],
        vec!["!**/tests/**/*"],
        vec![file("p-1/src/lib.rs", 10), file("p-1/src/tests/t.rs", 20)],
    );
    assert_eq!(include, vec!["src/**/*", "!**/tests/**/*"]);
    assert_eq!(waste, vec![file("p-1/src/tests/t.rs", 20)]);
}

#[test]
fn wildcard_patterns_reorder_too() {
    let (include, waste) = merged(
        vec!["src/**/*", "data/file?.bin", "assets/icon[0-9].png"],
        vec!["!data/**/*", "!assets/**/*"],
        vec![
            file("p-1/src/lib.rs", 10),
            file("p-1/data/file1.bin", 20),
            file("p-1/assets/icon5.png", 30),
        ],
    );
    assert_eq!(
        include,
        vec![
            "src/**/*",
            "!data/**/*",
            "!assets/**/*",
            "data/file?.bin",
            "assets/icon[0-9].png"
        ]
    );
    assert_eq!(waste, vec![]);
}

#[test]
fn unmatched_patterns_stay() {
    let (include, waste) = merged(
        vec!["src/**/*", "LICENSE"],
        vec!["!**/tests/**/*"],
        vec![file("p-1/src/lib.rs", 10)],
    );
    assert_eq!(include, vec!["src/**/*", "LICENSE", "!**/tests/**/*"]);
    assert_eq!(waste, vec![]);
}

#[test]
fn multi_file_patterns_are_not_rescued() {
    let (include, waste) = merged(
        vec!["examples/**/*.rs"],
        vec!["!**/examples/**/*"],
        vec![file("p-1/examples/a.rs", 10), file("p-1/examples/b.rs", 20)],
    );
    assert_eq!(include, vec!["examples/**/*.rs", "!**/examples/**/*"]);
    assert_eq!(
        waste,
        vec![file("p-1/examples/a.rs", 10), file("p-1/examples/b.rs", 20)]
    );
}

#[test]
fn reinclude_after_directory_negation_stays() {
    let (include, waste) = merged(
        vec!["src/**/*", "!examples/**", "examples/example.rs"],
        vec![],
        vec![file("p-1/src/lib.rs", 10), file("p-1/examples/example.rs", 455)],
    );
    assert_eq!(include, vec!["src/**/*", "!examples/**", "examples/example.rs"]);
    assert_eq!(waste, vec![]);
}

#[test]
fn waste_is_what_final_patterns_leave_out() {
    let (include, waste) = merged(
        vec!["src/**/*"],
        vec!["!**/tests/**/*"],
        vec![
            file("p-1/Cargo.toml", 5),
            file("p-1/benches/b.rs", 15),
            file("p-1/src/lib.rs", 10),
            file("p-1/src/tests/t.rs", 20),
        ],
    );
    assert_eq!(include, vec!["src/**/*", "!**/tests/**/*"]);
    assert_eq!(
        waste,
        vec![file("p-1/benches/b.rs", 15), file("p-1/src/tests/t.rs", 20)]
    );
}
