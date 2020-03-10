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

#[derive(Deserialize)]
struct Package {
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    build: Option<String>,
}
#[derive(Deserialize)]
struct CargoConfig {
    package: Package,
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
) -> (Vec<TarHeader>, Vec<TarHeader>) {
    // Shortcut: we assume '/' as path separator, which is true for all paths in crates.io except for 214 :D - it's OK to not find things in that case.
    let globs = globset_from(directories.iter().map(|e| {
        let mut s = tar_path_to_utf8_str(&e.path).to_string();
        s.push_str("/**");
        s
    }))
    .expect("always valid globs from directories");
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
        let glob = globset::Glob::new(&pattern).expect("valid include patterns");
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

fn simplify_standard_includes(
    includes: &'static [&'static str],
    entries: &[TarHeader],
) -> Patterns {
    let mut out_patterns: Vec<_> = includes
        .iter()
        .filter(|p| p.contains("**"))
        .map(|p| p.to_string())
        .collect();
    for pattern in includes.iter().filter(|p| !p.contains("**")) {
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

impl Report {
    fn package_from_entries(entries: &[(TarHeader, Vec<u8>)]) -> Package {
        find_in_entries(entries, &[], "Cargo.toml")
            .and_then(|(_e, v)| {
                v.map(|v| {
                    toml::from_slice::<CargoConfig>(&v)
                        .expect("valid Cargo.toml format")
                        .package
                })
            })
            .expect("Cargo.toml to always be present in the exploded crate")
    }

    fn convert_to_wasted_files(entries: Vec<TarHeader>) -> Vec<WastedFile> {
        entries
            .into_iter()
            .map(|e| (tar_path_to_utf8_str(&e.path).to_owned(), e.size))
            .collect()
    }

    fn standard_includes(
        _entries: Vec<TarHeader>,
        _build: Option<String>,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let include_patterns = standard_include_patterns();
        let include_globs = globset_from(include_patterns).expect("always valid include patterns");
        let (included_entries, excluded_entries) =
            split_to_matched_and_unmatched(_entries, &include_globs);
        let include_patterns = simplify_standard_includes(include_patterns, &included_entries);

        (
            Some(Fix::NewInclude {
                include: include_patterns,
                has_build_script: false,
            }),
            excluded_entries,
        )
    }

    fn compute_includes_from_includes_and_excludes(
        entries: Vec<TarHeader>,
        include_patterns: Vec<String>,
        exclude_patterns: Vec<String>,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let exclude_globs =
            globset_from(&exclude_patterns).expect("only valid exclude globs in Cargo.toml");
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
                    include_patterns,
                );
            Fix::RemoveExcludeAndUseInclude {
                include_added,
                include,
                include_removed,
            }
        };

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
                let (suggested_fix, wasted_files) =
                    match (package.include, package.exclude, package.build) {
                        (Some(includes), Some(excludes), _build_script_does_not_matter) => {
                            Self::compute_includes_from_includes_and_excludes(
                                entries_meta_data,
                                includes,
                                excludes,
                            )
                        }
                        (Some(_includes), None, _build) => unimplemented!(
                        "allow everything, assuming they know what they are doing, but flag tests"
                    ),
                        (None, Some(_excludes), _build) => {
                            unimplemented!("check for accidental includes")
                        }
                        (None, None, build) => Self::standard_includes(entries_meta_data, build),
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

#[cfg(test)]
mod from_extract_crate {
    use super::{Fix, Report};
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
                wasted_files: [("pregenerated/tmp/aes-586-win32n.asm", 25423u64), ("pregenerated/tmp/aes-x86_64-nasm.asm", 25697), ("pregenerated/tmp/aesni-gcm-x86_64-nasm.asm", 22260), ("pregenerated/tmp/aesni-x86-win32n.asm", 13074), ("pregenerated/tmp/aesni-x86_64-nasm.asm", 24852), ("pregenerated/tmp/chacha-x86-win32n.asm", 18916), ("pregenerated/tmp/chacha-x86_64-nasm.asm", 40140), ("pregenerated/tmp/ecp_nistz256-x86-win32n.asm", 31016), ("pregenerated/tmp/ghash-x86-win32n.asm", 19662), ("pregenerated/tmp/ghash-x86_64-nasm.asm", 39583), ("pregenerated/tmp/p256-x86_64-asm-nasm.asm", 82748), ("pregenerated/tmp/p256_beeu-x86_64-asm-nasm.asm", 4358), ("pregenerated/tmp/poly1305-x86-win32n.asm", 25445), ("pregenerated/tmp/poly1305-x86_64-nasm.asm", 39449), ("pregenerated/tmp/sha256-586-win32n.asm", 91985), ("pregenerated/tmp/sha256-x86_64-nasm.asm", 90321), ("pregenerated/tmp/sha512-586-win32n.asm", 38150), ("pregenerated/tmp/sha512-x86_64-nasm.asm", 70857), ("pregenerated/tmp/vpaes-x86-win32n.asm", 8142), ("pregenerated/tmp/vpaes-x86_64-nasm.asm", 10157), ("pregenerated/tmp/x86-mont-win32n.asm", 4312), ("pregenerated/tmp/x86_64-mont-nasm.asm", 23026), ("pregenerated/tmp/x86_64-mont5-nasm.asm", 64107)].iter().map(|(p,s)|(p.to_string(), *s)).collect(),
                suggested_fix: Some(Fix::RemoveExcludeAndUseInclude {
                    include_added: ["pregenerated/aes-586-elf.S", "pregenerated/aes-586-macosx.S", "pregenerated/aes-586-win32n.obj", "pregenerated/aes-armv4-ios32.S", "pregenerated/aes-armv4-linux32.S", "pregenerated/aes-x86_64-elf.S", "pregenerated/aes-x86_64-macosx.S", "pregenerated/aes-x86_64-nasm.obj", "pregenerated/aesni-gcm-x86_64-elf.S", "pregenerated/aesni-gcm-x86_64-macosx.S", "pregenerated/aesni-gcm-x86_64-nasm.obj", "pregenerated/aesni-x86-elf.S", "pregenerated/aesni-x86-macosx.S", "pregenerated/aesni-x86-win32n.obj", "pregenerated/aesni-x86_64-elf.S", "pregenerated/aesni-x86_64-macosx.S", "pregenerated/aesni-x86_64-nasm.obj", "pregenerated/aesv8-armx-ios32.S", "pregenerated/aesv8-armx-ios64.S", "pregenerated/aesv8-armx-linux32.S", "pregenerated/aesv8-armx-linux64.S", "pregenerated/armv4-mont-ios32.S", "pregenerated/armv4-mont-linux32.S", "pregenerated/armv8-mont-ios64.S", "pregenerated/armv8-mont-linux64.S", "pregenerated/bsaes-armv7-ios32.S", "pregenerated/bsaes-armv7-linux32.S", "pregenerated/chacha-armv4-ios32.S", "pregenerated/chacha-armv4-linux32.S", "pregenerated/chacha-armv8-ios64.S", "pregenerated/chacha-armv8-linux64.S", "pregenerated/chacha-x86-elf.S", "pregenerated/chacha-x86-macosx.S", "pregenerated/chacha-x86-win32n.obj", "pregenerated/chacha-x86_64-elf.S", "pregenerated/chacha-x86_64-macosx.S", "pregenerated/chacha-x86_64-nasm.obj", "pregenerated/ecp_nistz256-armv4-ios32.S", "pregenerated/ecp_nistz256-armv4-linux32.S", "pregenerated/ecp_nistz256-armv8-ios64.S", "pregenerated/ecp_nistz256-armv8-linux64.S", "pregenerated/ecp_nistz256-x86-elf.S", "pregenerated/ecp_nistz256-x86-macosx.S", "pregenerated/ecp_nistz256-x86-win32n.obj", "pregenerated/ghash-armv4-ios32.S", "pregenerated/ghash-armv4-linux32.S", "pregenerated/ghash-x86-elf.S", "pregenerated/ghash-x86-macosx.S", "pregenerated/ghash-x86-win32n.obj", "pregenerated/ghash-x86_64-elf.S", "pregenerated/ghash-x86_64-macosx.S", "pregenerated/ghash-x86_64-nasm.obj", "pregenerated/ghashv8-armx-ios32.S", "pregenerated/ghashv8-armx-ios64.S", "pregenerated/ghashv8-armx-linux32.S", "pregenerated/ghashv8-armx-linux64.S", "pregenerated/p256-x86_64-asm-elf.S", "pregenerated/p256-x86_64-asm-macosx.S", "pregenerated/p256-x86_64-asm-nasm.obj", "pregenerated/p256_beeu-x86_64-asm-elf.S", "pregenerated/p256_beeu-x86_64-asm-macosx.S", "pregenerated/p256_beeu-x86_64-asm-nasm.obj", "pregenerated/poly1305-armv4-ios32.S", "pregenerated/poly1305-armv4-linux32.S", "pregenerated/poly1305-armv8-ios64.S", "pregenerated/poly1305-armv8-linux64.S", "pregenerated/poly1305-x86-elf.S", "pregenerated/poly1305-x86-macosx.S", "pregenerated/poly1305-x86-win32n.obj", "pregenerated/poly1305-x86_64-elf.S", "pregenerated/poly1305-x86_64-macosx.S", "pregenerated/poly1305-x86_64-nasm.obj", "pregenerated/sha256-586-elf.S", "pregenerated/sha256-586-macosx.S", "pregenerated/sha256-586-win32n.obj", "pregenerated/sha256-armv4-ios32.S", "pregenerated/sha256-armv4-linux32.S", "pregenerated/sha256-armv8-ios64.S", "pregenerated/sha256-armv8-linux64.S", "pregenerated/sha256-x86_64-elf.S", "pregenerated/sha256-x86_64-macosx.S", "pregenerated/sha256-x86_64-nasm.obj", "pregenerated/sha512-586-elf.S", "pregenerated/sha512-586-macosx.S", "pregenerated/sha512-586-win32n.obj", "pregenerated/sha512-armv4-ios32.S", "pregenerated/sha512-armv4-linux32.S", "pregenerated/sha512-armv8-ios64.S", "pregenerated/sha512-armv8-linux64.S", "pregenerated/sha512-x86_64-elf.S", "pregenerated/sha512-x86_64-macosx.S", "pregenerated/sha512-x86_64-nasm.obj", "pregenerated/vpaes-x86-elf.S", "pregenerated/vpaes-x86-macosx.S", "pregenerated/vpaes-x86-win32n.obj", "pregenerated/vpaes-x86_64-elf.S", "pregenerated/vpaes-x86_64-macosx.S", "pregenerated/vpaes-x86_64-nasm.obj", "pregenerated/x86-mont-elf.S", "pregenerated/x86-mont-macosx.S", "pregenerated/x86-mont-win32n.obj", "pregenerated/x86_64-mont-elf.S", "pregenerated/x86_64-mont-macosx.S", "pregenerated/x86_64-mont-nasm.obj", "pregenerated/x86_64-mont5-elf.S", "pregenerated/x86_64-mont5-macosx.S", "pregenerated/x86_64-mont5-nasm.obj"].iter().map(|s| s.to_string()).collect(),
                    include: ["LICENSE", "pregenerated/aes-586-elf.S", "pregenerated/aes-586-macosx.S", "pregenerated/aes-586-win32n.obj", "pregenerated/aes-armv4-ios32.S", "pregenerated/aes-armv4-linux32.S", "pregenerated/aes-x86_64-elf.S", "pregenerated/aes-x86_64-macosx.S", "pregenerated/aes-x86_64-nasm.obj", "pregenerated/aesni-gcm-x86_64-elf.S", "pregenerated/aesni-gcm-x86_64-macosx.S", "pregenerated/aesni-gcm-x86_64-nasm.obj", "pregenerated/aesni-x86-elf.S", "pregenerated/aesni-x86-macosx.S", "pregenerated/aesni-x86-win32n.obj", "pregenerated/aesni-x86_64-elf.S", "pregenerated/aesni-x86_64-macosx.S", "pregenerated/aesni-x86_64-nasm.obj", "pregenerated/aesv8-armx-ios32.S", "pregenerated/aesv8-armx-ios64.S", "pregenerated/aesv8-armx-linux32.S", "pregenerated/aesv8-armx-linux64.S", "pregenerated/armv4-mont-ios32.S", "pregenerated/armv4-mont-linux32.S", "pregenerated/armv8-mont-ios64.S", "pregenerated/armv8-mont-linux64.S", "pregenerated/bsaes-armv7-ios32.S", "pregenerated/bsaes-armv7-linux32.S", "pregenerated/chacha-armv4-ios32.S", "pregenerated/chacha-armv4-linux32.S", "pregenerated/chacha-armv8-ios64.S", "pregenerated/chacha-armv8-linux64.S", "pregenerated/chacha-x86-elf.S", "pregenerated/chacha-x86-macosx.S", "pregenerated/chacha-x86-win32n.obj", "pregenerated/chacha-x86_64-elf.S", "pregenerated/chacha-x86_64-macosx.S", "pregenerated/chacha-x86_64-nasm.obj", "pregenerated/ecp_nistz256-armv4-ios32.S", "pregenerated/ecp_nistz256-armv4-linux32.S", "pregenerated/ecp_nistz256-armv8-ios64.S", "pregenerated/ecp_nistz256-armv8-linux64.S", "pregenerated/ecp_nistz256-x86-elf.S", "pregenerated/ecp_nistz256-x86-macosx.S", "pregenerated/ecp_nistz256-x86-win32n.obj", "pregenerated/ghash-armv4-ios32.S", "pregenerated/ghash-armv4-linux32.S", "pregenerated/ghash-x86-elf.S", "pregenerated/ghash-x86-macosx.S", "pregenerated/ghash-x86-win32n.obj", "pregenerated/ghash-x86_64-elf.S", "pregenerated/ghash-x86_64-macosx.S", "pregenerated/ghash-x86_64-nasm.obj", "pregenerated/ghashv8-armx-ios32.S", "pregenerated/ghashv8-armx-ios64.S", "pregenerated/ghashv8-armx-linux32.S", "pregenerated/ghashv8-armx-linux64.S", "pregenerated/p256-x86_64-asm-elf.S", "pregenerated/p256-x86_64-asm-macosx.S", "pregenerated/p256-x86_64-asm-nasm.obj", "pregenerated/p256_beeu-x86_64-asm-elf.S", "pregenerated/p256_beeu-x86_64-asm-macosx.S", "pregenerated/p256_beeu-x86_64-asm-nasm.obj", "pregenerated/poly1305-armv4-ios32.S", "pregenerated/poly1305-armv4-linux32.S", "pregenerated/poly1305-armv8-ios64.S", "pregenerated/poly1305-armv8-linux64.S", "pregenerated/poly1305-x86-elf.S", "pregenerated/poly1305-x86-macosx.S", "pregenerated/poly1305-x86-win32n.obj", "pregenerated/poly1305-x86_64-elf.S", "pregenerated/poly1305-x86_64-macosx.S", "pregenerated/poly1305-x86_64-nasm.obj", "pregenerated/sha256-586-elf.S", "pregenerated/sha256-586-macosx.S", "pregenerated/sha256-586-win32n.obj", "pregenerated/sha256-armv4-ios32.S", "pregenerated/sha256-armv4-linux32.S", "pregenerated/sha256-armv8-ios64.S", "pregenerated/sha256-armv8-linux64.S", "pregenerated/sha256-x86_64-elf.S", "pregenerated/sha256-x86_64-macosx.S", "pregenerated/sha256-x86_64-nasm.obj", "pregenerated/sha512-586-elf.S", "pregenerated/sha512-586-macosx.S", "pregenerated/sha512-586-win32n.obj", "pregenerated/sha512-armv4-ios32.S", "pregenerated/sha512-armv4-linux32.S", "pregenerated/sha512-armv8-ios64.S", "pregenerated/sha512-armv8-linux64.S", "pregenerated/sha512-x86_64-elf.S", "pregenerated/sha512-x86_64-macosx.S", "pregenerated/sha512-x86_64-nasm.obj", "pregenerated/vpaes-x86-elf.S", "pregenerated/vpaes-x86-macosx.S", "pregenerated/vpaes-x86-win32n.obj", "pregenerated/vpaes-x86_64-elf.S", "pregenerated/vpaes-x86_64-macosx.S", "pregenerated/vpaes-x86_64-nasm.obj", "pregenerated/x86-mont-elf.S", "pregenerated/x86-mont-macosx.S", "pregenerated/x86-mont-win32n.obj", "pregenerated/x86_64-mont-elf.S", "pregenerated/x86_64-mont-macosx.S", "pregenerated/x86_64-mont-nasm.obj", "pregenerated/x86_64-mont5-elf.S", "pregenerated/x86_64-mont5-macosx.S", "pregenerated/x86_64-mont5-nasm.obj", "build.rs", "crypto/block.c", "crypto/block.h", "crypto/chacha/asm/chacha-armv4.pl", "crypto/chacha/asm/chacha-armv8.pl", "crypto/chacha/asm/chacha-x86.pl", "crypto/chacha/asm/chacha-x86_64.pl", "crypto/cipher_extra/asm/aes128gcmsiv-x86_64.pl", "crypto/cipher_extra/test/aes_128_gcm_siv_tests.txt", "crypto/cipher_extra/test/aes_256_gcm_siv_tests.txt", "crypto/constant_time_test.c", "crypto/cpu-aarch64-linux.c", "crypto/cpu-arm-linux.c", "crypto/cpu-arm.c", "crypto/cpu-intel.c", "crypto/crypto.c", "crypto/curve25519/asm/x25519-asm-arm.S", "crypto/fipsmodule/aes/aes.c", "crypto/fipsmodule/aes/asm/aes-586.pl", "crypto/fipsmodule/aes/asm/aes-armv4.pl", "crypto/fipsmodule/aes/asm/aes-x86_64.pl", "crypto/fipsmodule/aes/asm/aesni-x86.pl", "crypto/fipsmodule/aes/asm/aesni-x86_64.pl", "crypto/fipsmodule/aes/asm/aesv8-armx.pl", "crypto/fipsmodule/aes/asm/bsaes-armv7.pl", "crypto/fipsmodule/aes/asm/bsaes-x86_64.pl", "crypto/fipsmodule/aes/asm/vpaes-x86.pl", "crypto/fipsmodule/aes/asm/vpaes-x86_64.pl", "crypto/fipsmodule/aes/internal.h", "crypto/fipsmodule/bn/asm/armv4-mont.pl", "crypto/fipsmodule/bn/asm/armv8-mont.pl", "crypto/fipsmodule/bn/asm/x86-mont.pl", "crypto/fipsmodule/bn/asm/x86_64-mont.pl", "crypto/fipsmodule/bn/asm/x86_64-mont5.pl", "crypto/fipsmodule/bn/generic.c", "crypto/fipsmodule/bn/internal.h", "crypto/fipsmodule/bn/montgomery.c", "crypto/fipsmodule/bn/montgomery_inv.c", "crypto/fipsmodule/cipher/e_aes.c", "crypto/fipsmodule/ec/asm/ecp_nistz256-armv4.pl", "crypto/fipsmodule/ec/asm/ecp_nistz256-armv8.pl", "crypto/fipsmodule/ec/asm/ecp_nistz256-x86.pl", "crypto/fipsmodule/ec/asm/p256-x86_64-asm.pl", "crypto/fipsmodule/ec/ecp_nistz.c", "crypto/fipsmodule/ec/ecp_nistz.h", "crypto/fipsmodule/ec/ecp_nistz256.c", "crypto/fipsmodule/ec/ecp_nistz256.h", "crypto/fipsmodule/ec/ecp_nistz256_table.inl", "crypto/fipsmodule/ec/ecp_nistz384.h", "crypto/fipsmodule/ec/ecp_nistz384.inl", "crypto/fipsmodule/ec/gfp_p256.c", "crypto/fipsmodule/ec/gfp_p384.c", "crypto/fipsmodule/ecdsa/ecdsa_verify_tests.txt", "crypto/fipsmodule/modes/asm/aesni-gcm-x86_64.pl", "crypto/fipsmodule/modes/asm/ghash-armv4.pl", "crypto/fipsmodule/modes/asm/ghash-x86.pl", "crypto/fipsmodule/modes/asm/ghash-x86_64.pl", "crypto/fipsmodule/modes/asm/ghashv8-armx.pl", "crypto/fipsmodule/modes/gcm.c", "crypto/fipsmodule/modes/internal.h", "crypto/fipsmodule/sha/asm/sha256-586.pl", "crypto/fipsmodule/sha/asm/sha256-armv4.pl", "crypto/fipsmodule/sha/asm/sha512-586.pl", "crypto/fipsmodule/sha/asm/sha512-armv4.pl", "crypto/fipsmodule/sha/asm/sha512-armv8.pl", "crypto/fipsmodule/sha/asm/sha512-x86_64.pl", "crypto/internal.h", "crypto/limbs/limbs.c", "crypto/limbs/limbs.h", "crypto/limbs/limbs.inl", "crypto/mem.c", "crypto/perlasm/arm-xlate.pl", "crypto/perlasm/x86asm.pl", "crypto/perlasm/x86gas.pl", "crypto/perlasm/x86nasm.pl", "crypto/perlasm/x86_64-xlate.pl", "crypto/poly1305/asm/poly1305-armv4.pl", "crypto/poly1305/asm/poly1305-armv8.pl", "crypto/poly1305/asm/poly1305-x86.pl", "crypto/poly1305/asm/poly1305-x86_64.pl", "examples/checkdigest.rs", "include/GFp/aes.h", "include/GFp/arm_arch.h", "include/GFp/base.h", "include/GFp/cpu.h", "include/GFp/mem.h", "include/GFp/type_check.h", "src/aead.rs", "src/aead/aes.rs", "src/aead/aes_gcm.rs", "src/aead/aes_tests.txt", "src/aead/block.rs", "src/aead/chacha.rs", "src/aead/chacha_tests.txt", "src/aead/chacha20_poly1305.rs", "src/aead/chacha20_poly1305_openssh.rs", "src/aead/gcm.rs", "src/aead/nonce.rs", "src/aead/poly1305.rs", "src/aead/poly1305_test.txt", "src/aead/shift.rs", "src/agreement.rs", "src/arithmetic.rs", "src/arithmetic/montgomery.rs", "src/array.rs", "src/bits.rs", "src/bssl.rs", "src/c.rs", "src/constant_time.rs", "src/cpu.rs", "src/data/alg-rsa-encryption.der", "src/debug.rs", "src/digest.rs", "src/digest/sha1.rs", "src/ec/curve25519/ed25519/digest.rs", "src/ec/curve25519/ed25519.rs", "src/ec/curve25519/ed25519/signing.rs", "src/ec/curve25519/ed25519/verification.rs", "src/ec/curve25519/ed25519/ed25519_pkcs8_v2_template.der", "src/ec/curve25519.rs", "src/ec/curve25519/ops.rs", "src/ec/curve25519/x25519.rs", "src/ec.rs", "src/ec/keys.rs", "src/ec/suite_b/curve.rs", "src/ec/suite_b/ecdh.rs", "src/ec/suite_b/ecdsa/digest_scalar.rs", "src/ec/suite_b/ecdsa.rs", "src/ec/suite_b/ecdsa/signing.rs", "src/ec/suite_b/ecdsa/verification.rs", "src/ec/suite_b/ecdsa/ecdsa_digest_scalar_tests.txt", "src/ec/suite_b/ecdsa/ecPublicKey_p256_pkcs8_v1_template.der", "src/ec/suite_b/ecdsa/ecPublicKey_p384_pkcs8_v1_template.der", "src/ec/suite_b/ecdsa/ecdsa_sign_asn1_tests.txt", "src/ec/suite_b/ecdsa/ecdsa_sign_fixed_tests.txt", "src/ec/suite_b.rs", "src/ec/suite_b/ops/elem.rs", "src/ec/suite_b/ops.rs", "src/ec/suite_b/ops/p256.rs", "src/ec/suite_b/ops/p256_elem_mul_tests.txt", "src/ec/suite_b/ops/p256_elem_neg_tests.txt", "src/ec/suite_b/ops/p256_elem_sum_tests.txt", "src/ec/suite_b/ops/p256_point_double_tests.txt", "src/ec/suite_b/ops/p256_point_mul_base_tests.txt", "src/ec/suite_b/ops/p256_point_mul_serialized_tests.txt", "src/ec/suite_b/ops/p256_point_mul_tests.txt", "src/ec/suite_b/ops/p256_point_sum_mixed_tests.txt", "src/ec/suite_b/ops/p256_point_sum_tests.txt", "src/ec/suite_b/ops/p256_scalar_mul_tests.txt", "src/ec/suite_b/ops/p256_scalar_square_tests.txt", "src/ec/suite_b/ops/p384.rs", "src/ec/suite_b/ops/p384_elem_div_by_2_tests.txt", "src/ec/suite_b/ops/p384_elem_mul_tests.txt", "src/ec/suite_b/ops/p384_elem_neg_tests.txt", "src/ec/suite_b/ops/p384_elem_sum_tests.txt", "src/ec/suite_b/ops/p384_point_double_tests.txt", "src/ec/suite_b/ops/p384_point_mul_base_tests.txt", "src/ec/suite_b/ops/p384_point_mul_tests.txt", "src/ec/suite_b/ops/p384_point_sum_tests.txt", "src/ec/suite_b/ops/p384_scalar_mul_tests.txt", "src/ec/suite_b/private_key.rs", "src/ec/suite_b/public_key.rs", "src/ec/suite_b/suite_b_public_key_tests.txt", "src/endian.rs", "src/error.rs", "src/hkdf.rs", "src/hmac.rs", "src/hmac_generate_serializable_tests.txt", "src/io.rs", "src/io/der.rs", "src/io/der_writer.rs", "src/io/writer.rs", "src/lib.rs", "src/limb.rs", "src/endian.rs", "src/pbkdf2.rs", "src/pkcs8.rs", "src/polyfill.rs", "src/polyfill/convert.rs", "src/rand.rs", "src/rsa/bigint.rs", "src/rsa/bigint_elem_exp_consttime_tests.txt", "src/rsa/bigint_elem_exp_vartime_tests.txt", "src/rsa/bigint_elem_mul_tests.txt", "src/rsa/bigint_elem_reduced_once_tests.txt", "src/rsa/bigint_elem_reduced_tests.txt", "src/rsa/bigint_elem_squared_tests.txt", "src/rsa/convert_nist_rsa_test_vectors.py", "src/rsa.rs", "src/rsa/padding.rs", "src/rsa/random.rs", "src/rsa/rsa_pss_padding_tests.txt", "src/rsa/signature_rsa_example_private_key.der", "src/rsa/signature_rsa_example_public_key.der", "src/rsa/signing.rs", "src/rsa/verification.rs", "src/signature.rs", "src/test.rs", "src/test_1_syntax_error_tests.txt", "src/test_1_tests.txt", "src/test_3_tests.txt", "tests/aead_aes_128_gcm_tests.txt", "tests/aead_aes_256_gcm_tests.txt", "tests/aead_chacha20_poly1305_tests.txt", "tests/aead_chacha20_poly1305_openssh_tests.txt", "tests/aead_tests.rs", "tests/agreement_tests.rs", "tests/agreement_tests.txt", "tests/digest_tests.rs", "tests/digest_tests.txt", "tests/ecdsa_from_pkcs8_tests.txt", "tests/ecdsa_tests.rs", "tests/ecdsa_sign_asn1_tests.txt", "tests/ecdsa_sign_fixed_tests.txt", "tests/ecdsa_verify_asn1_tests.txt", "tests/ecdsa_verify_fixed_tests.txt", "tests/ed25519_from_pkcs8_tests.txt", "tests/ed25519_from_pkcs8_unchecked_tests.txt", "tests/ed25519_tests.rs", "tests/ed25519_tests.txt", "tests/ed25519_test_private_key.bin", "tests/ed25519_test_public_key.bin", "tests/hkdf_tests.rs", "tests/hkdf_tests.txt", "tests/hmac_tests.rs", "tests/hmac_tests.txt", "tests/pbkdf2_tests.rs", "tests/pbkdf2_tests.txt", "tests/rsa_from_pkcs8_tests.txt", "tests/rsa_pkcs1_sign_tests.txt", "tests/rsa_pkcs1_verify_tests.txt", "tests/rsa_primitive_verify_tests.txt", "tests/rsa_pss_sign_tests.txt", "tests/rsa_pss_verify_tests.txt", "tests/rsa_tests.rs", "tests/signature_tests.rs", "third_party/fiat/curve25519.c", "third_party/fiat/curve25519_tables.h", "third_party/fiat/internal.h", "third_party/fiat/LICENSE", "third_party/fiat/make_curve25519_tables.py", "third_party/NIST/SHAVS/SHA1LongMsg.rsp", "third_party/NIST/SHAVS/SHA1Monte.rsp", "third_party/NIST/SHAVS/SHA1ShortMsg.rsp", "third_party/NIST/SHAVS/SHA224LongMsg.rsp", "third_party/NIST/SHAVS/SHA224Monte.rsp", "third_party/NIST/SHAVS/SHA224ShortMsg.rsp", "third_party/NIST/SHAVS/SHA256LongMsg.rsp", "third_party/NIST/SHAVS/SHA256Monte.rsp", "third_party/NIST/SHAVS/SHA256ShortMsg.rsp", "third_party/NIST/SHAVS/SHA384LongMsg.rsp", "third_party/NIST/SHAVS/SHA384Monte.rsp", "third_party/NIST/SHAVS/SHA384ShortMsg.rsp", "third_party/NIST/SHAVS/SHA512LongMsg.rsp", "third_party/NIST/SHAVS/SHA512Monte.rsp", "third_party/NIST/SHAVS/SHA512ShortMsg.rsp"].iter().map(|s|s.to_string()).collect(),
                    include_removed: vec!["pregenerated/*".into(), "Cargo.toml".into()]
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
                wasted_files: [(".gitignore", 40u64), ("Jenkinsfile", 3735), ("Specs/libsovrin-core/0.0.1/libsovrin-core.podspec.json", 529), ("Specs/libsovrin-core/0.0.2/libsovrin-core.podspec.json", 529), ("Specs/libzmq/4.2.2/libzmq.podspec.json", 1545), ("Specs/libzmq/4.2.3/libzmq.podspec.json", 1555), ("Specs/milagro/3.0.0/milagro.podspec.json", 963), ("build-libsovrin-core-ios.sh", 1368), ("build.rs", 1033), ("ci/amazon.dockerfile", 1492), ("ci/sovrin-pool.dockerfile", 2678), ("ci/ubuntu.dockerfile", 847), ("ci/update-package-version.sh", 175), ("doc/ios-build.md", 1514), ("doc/libsovrin-agent-2-agent.puml", 2168), ("doc/libsovrin-anoncreds.puml", 3558), ("doc/mac-build.md", 597), ("docker-compose.yml", 323), ("include/sovrin_agent.h", 9229), ("include/sovrin_anoncreds.h", 11254), ("include/sovrin_core.h", 265), ("include/sovrin_ledger.h", 15319), ("include/sovrin_mod.h", 2887), ("include/sovrin_pool.h", 2209), ("include/sovrin_signus.h", 11643), ("include/sovrin_types.h", 218), ("include/sovrin_wallet.h", 8548), ("tests/agent.rs", 13726), ("tests/anoncreds.rs", 229549), ("tests/demo.rs", 41150), ("tests/ledger.rs", 55607), ("tests/pool.rs", 16640), ("tests/signus.rs", 26106), ("tests/utils/agent.rs", 5569), ("tests/utils/anoncreds.rs", 27339), ("tests/utils/callback.rs", 45981), ("tests/utils/ledger.rs", 14993), ("tests/utils/mod.rs", 338), ("tests/utils/pool.rs", 8532), ("tests/utils/signus.rs", 8135), ("tests/utils/types.rs", 5085), ("tests/utils/wallet.rs", 7290), ("tests/wallet.rs", 10892), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/.gitignore", 607), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/Podfile", 1490), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test.xcodeproj/project.pbxproj", 17063), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test.xcodeproj/project.xcworkspace/contents.xcworkspacedata", 156), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test.xcworkspace/contents.xcworkspacedata", 229), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/AppDelegate.h", 293), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/AppDelegate.m", 2106), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/Assets.xcassets/AppIcon.appiconset/Contents.json", 1077), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/Base.lproj/LaunchScreen.storyboard", 1740), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/Base.lproj/Main.storyboard", 1689), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/Info.plist", 1442), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/ViewController.h", 231), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/ViewController.m", 512), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/ZeroMQTests.h", 243), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/ZeroMQTests.mm", 307742), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/main.m", 350), ("wrappers/ios/Tests/libzmq-ios-test-app/ZeroMQ_Test/ZeroMQ_Test/zhelpers.h", 5216), ("wrappers/ios/Tests/milagro-ios-test-app/.gitignore", 607), ("wrappers/ios/Tests/milagro-ios-test-app/Podfile", 789), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app.xcodeproj/project.pbxproj", 36318), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app.xcodeproj/project.xcworkspace/contents.xcworkspacedata", 161), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app.xcworkspace/contents.xcworkspacedata", 234), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/AppDelegate.h", 143), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/AppDelegate.m", 1956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/Assets.xcassets/AppIcon.appiconset/Contents.json", 1077), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/Base.lproj/LaunchScreen.storyboard", 1740), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/Base.lproj/Main.storyboard", 1689), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/Info.plist", 1442), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/MilagroTest.h", 248), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/MilagroTest.m", 7913), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/ViewController.h", 78), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/ViewController.m", 359), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/main.m", 355), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/aes/CBCMMT128.rsp", 9523), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/aes/CBCMMT256.rsp", 10163), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/aes/CFB8MMT128.rsp", 3055), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/aes/CFB8MMT256.rsp", 3695), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/aes/ECBMMT128.rsp", 8763), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/aes/ECBMMT256.rsp", 9403), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/aes/amcl_CTRMCL128.rsp", 9171), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/aes/amcl_CTRMCL256.rsp", 9810), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/big/test_vector_big.txt", 5510), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdh/C25519/KAS_ECC_CDH_PrimitiveTest.txt", 4851), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdh/P-256/KAS_ECC_CDH_PrimitiveTest.txt", 11248), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdh/P-384/KAS_ECC_CDH_PrimitiveTest.txt", 16048), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdh/P-521/KAS_ECC_CDH_PrimitiveTest.txt", 21448), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-256/KeyPair.rsp", 2167), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-256/sha256Sign.rsp", 10216), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-256/sha256Verify.rsp", 8536), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-256/sha512Sign.rsp", 10217), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-256/sha512Verify.rsp", 8536), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-384/KeyPair.rsp", 3126), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-384/sha256Sign.rsp", 13096), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-384/sha256Verify.rsp", 10456), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-384/sha384Sign.rsp", 13096), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-384/sha384Verify.rsp", 10456), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-384/sha512Sign.rsp", 13096), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-384/sha512Verify.rsp", 10456), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-521/KeyPair.rsp", 4207), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-521/sha256Sign.rsp", 16336), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-521/sha256Verify.rsp", 12616), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-521/sha512Sign.rsp", 16336), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecdsa/P-521/sha512Verify.rsp", 12616), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_ANSSI_WEIERSTRASS.txt", 18696), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_BLS383_WEIERSTRASS.txt", 27103), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_BLS455_WEIERSTRASS.txt", 31787), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_BN254_CX_WEIERSTRASS.txt", 18682), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_BN254_T2_WEIERSTRASS.txt", 18700), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_BN254_T_WEIERSTRASS.txt", 18694), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_BN254_WEIERSTRASS.txt", 18706), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_BN454_WEIERSTRASS.txt", 31793), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_BN646_WEIERSTRASS.txt", 44357), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_BRAINPOOL_WEIERSTRASS.txt", 18708), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_C25519_EDWARDS.txt", 18691), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_C25519_MONTGOMERY.txt", 5001), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_C41417_EDWARDS.txt", 29173), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_GOLDILOCKS_EDWARDS.txt", 31282), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_HIFIVE_EDWARDS.txt", 23942), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MF254_EDWARDS.txt", 18694), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MF254_MONTGOMERY.txt", 4997), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MF254_WEIERSTRASS.txt", 18696), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MF256_EDWARDS.txt", 18704), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MF256_MONTGOMERY.txt", 5000), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MF256_WEIERSTRASS.txt", 18701), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MS255_EDWARDS.txt", 18706), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MS255_MONTGOMERY.txt", 5002), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MS255_WEIERSTRASS.txt", 18702), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MS256_EDWARDS.txt", 18701), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MS256_MONTGOMERY.txt", 4997), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_MS256_WEIERSTRASS.txt", 18700), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_NIST256_WEIERSTRASS.txt", 18704), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_NIST384_WEIERSTRASS.txt", 27086), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp/test_vector_NIST521_WEIERSTRASS.txt", 36254), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp2/test_vector_BLS383_WEIERSTRASS.txt", 56679), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp2/test_vector_BLS455_WEIERSTRASS.txt", 66793), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp2/test_vector_BN254_CX_WEIERSTRASS.txt", 38670), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp2/test_vector_BN254_T2_WEIERSTRASS.txt", 38669), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp2/test_vector_BN254_T_WEIERSTRASS.txt", 38690), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp2/test_vector_BN254_WEIERSTRASS.txt", 38431), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp2/test_vector_BN454_WEIERSTRASS.txt", 66748), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/ecp2/test_vector_BN646_WEIERSTRASS.txt", 93741), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_ANSSI.txt", 7076), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_BLS383.txt", 10065), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_BLS455.txt", 11767), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_BN254.txt", 7269), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_BN254_CX.txt", 7094), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_BN254_T.txt", 6899), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_BN254_T2.txt", 6954), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_BN454.txt", 11201), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_BN646.txt", 16866), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_BRAINPOOL.txt", 7379), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_C25519.txt", 6965), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_C41417.txt", 11149), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_GOLDILOCKS.txt", 11983), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_HIFIVE.txt", 8645), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_MF254.txt", 6950), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_MF256.txt", 7161), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_MS255.txt", 6886), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_MS256.txt", 6994), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_NIST256.txt", 7100), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_NIST384.txt", 9807), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp/test_vector_NIST521.txt", 12986), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp2/test_vector_BLS383.txt", 32074), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp2/test_vector_BLS455.txt", 37672), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp2/test_vector_BN254.txt", 22121), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp2/test_vector_BN254_CX.txt", 22096), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp2/test_vector_BN254_T.txt", 22095), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp2/test_vector_BN254_T2.txt", 22112), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp2/test_vector_BN454.txt", 37658), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp2/test_vector_BN646.txt", 52589), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp4/test_vector_BLS383.txt", 79724), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp4/test_vector_BLS455.txt", 93832), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp4/test_vector_BN254.txt", 54617), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp4/test_vector_BN254_CX.txt", 54611), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp4/test_vector_BN254_T.txt", 54614), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp4/test_vector_BN254_T2.txt", 54609), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp4/test_vector_BN454.txt", 93798), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/fp4/test_vector_BN646.txt", 131433), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/gcm/gcmDecrypt128.rsp", 2682450), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/gcm/gcmDecrypt256.rsp", 2935620), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/gcm/gcmEncryptExtIV128.rsp", 2864783), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/gcm/gcmEncryptExtIV256.rsp", 3116783), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/mpin/BN254_CX.json", 992434), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/mpin/BN254_CX.txt", 917744), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/mpin/BN254_CXOnePass.json", 958264), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_ANSSI_WEIERSTRASS_32.txt", 968), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_ANSSI_WEIERSTRASS_64.txt", 956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BLS383_WEIERSTRASS_32.txt", 4769), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BLS383_WEIERSTRASS_64.txt", 4712), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BLS455_WEIERSTRASS_32.txt", 5567), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BLS455_WEIERSTRASS_64.txt", 5472), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_CX_WEIERSTRASS_16.txt", 3516), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_CX_WEIERSTRASS_32.txt", 3392), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_CX_WEIERSTRASS_64.txt", 3346), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_T2_WEIERSTRASS_16.txt", 3512), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_T2_WEIERSTRASS_32.txt", 3399), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_T2_WEIERSTRASS_64.txt", 3343), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_T_WEIERSTRASS_16.txt", 3373), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_T_WEIERSTRASS_32.txt", 3424), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_T_WEIERSTRASS_64.txt", 3377), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_WEIERSTRASS_16.txt", 3503), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_WEIERSTRASS_32.txt", 3393), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN254_WEIERSTRASS_64.txt", 3346), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN454_WEIERSTRASS_32.txt", 5484), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN454_WEIERSTRASS_64.txt", 5477), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN646_WEIERSTRASS_32.txt", 7666), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BN646_WEIERSTRASS_64.txt", 7509), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BRAINPOOL_WEIERSTRASS_32.txt", 964), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_BRAINPOOL_WEIERSTRASS_64.txt", 956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_C25519_EDWARDS_16.txt", 1005), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_C25519_EDWARDS_32.txt", 969), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_C25519_EDWARDS_64.txt", 956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_C25519_MONTGOMERY_32.txt", 839), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_C25519_MONTGOMERY_64.txt", 826), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_C41417_EDWARDS_32.txt", 1310), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_C41417_EDWARDS_64.txt", 1282), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_GOLDILOCKS_EDWARDS_32.txt", 1377), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_GOLDILOCKS_EDWARDS_64.txt", 1349), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_HIFIVE_EDWARDS_32.txt", 1134), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_HIFIVE_EDWARDS_64.txt", 1111), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF254_EDWARDS_32.txt", 970), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF254_EDWARDS_64.txt", 956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF254_MONTGOMERY_32.txt", 840), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF254_MONTGOMERY_64.txt", 826), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF254_WEIERSTRASS_32.txt", 970), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF254_WEIERSTRASS_64.txt", 956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF256_EDWARDS_32.txt", 967), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF256_EDWARDS_64.txt", 956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF256_MONTGOMERY_32.txt", 837), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF256_MONTGOMERY_64.txt", 1007), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF256_WEIERSTRASS_32.txt", 967), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MF256_WEIERSTRASS_64.txt", 956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS255_EDWARDS_32.txt", 971), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS255_EDWARDS_64.txt", 956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS255_MONTGOMERY_32.txt", 841), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS255_MONTGOMERY_64.txt", 826), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS255_WEIERSTRASS_32.txt", 971), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS255_WEIERSTRASS_64.txt", 956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS256_EDWARDS_32.txt", 969), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS256_EDWARDS_64.txt", 956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS256_MONTGOMERY_32.txt", 1020), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS256_MONTGOMERY_64.txt", 1007), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS256_WEIERSTRASS_32.txt", 969), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_MS256_WEIERSTRASS_64.txt", 956), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_NIST256_WEIERSTRASS_16.txt", 1006), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_NIST256_WEIERSTRASS_32.txt", 969), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_NIST256_WEIERSTRASS_64.txt", 955), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_NIST384_WEIERSTRASS_32.txt", 1233), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_NIST384_WEIERSTRASS_64.txt", 1217), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_NIST521_WEIERSTRASS_32.txt", 1518), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/output/test_vector_NIST521_WEIERSTRASS_64.txt", 1494), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/rsa/2048/pkcs-vect.txt", 10614), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/rsa/3072/pkcs-vect.txt", 18450), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/rsa/4096/pkcs-vect.txt", 22362), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/sha/256/SHA256ShortMsg.rsp", 10030), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/sha/384/SHA384ShortMsg.rsp", 12088), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/sha/512/SHA512ShortMsg.rsp", 36275), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/x509/2048_P256/pkits-vect.txt", 296267), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/x509/2048_P256/x509-vect.txt", 22330), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/x509/3072_P384/x509-vect.txt", 27205), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/x509/4096/x509-vect.txt", 12040), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/testVectors/x509/P521/x509-vect.txt", 5635), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_aes_decrypt.c", 6597), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_aes_encrypt.c", 6577), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_big_arithmetics.c", 10901), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_big_consistency.c", 6418), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_ecc.c", 6132), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_ecdh.c", 8189), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_ecdsa_keypair.c", 4459), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_ecdsa_sign.c", 8252), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_ecdsa_verify.c", 6672), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_ecp2_arithmetics.c", 13226), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_ecp_arithmetics.c", 14398), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_fp2_arithmetics.c", 14205), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_fp4_arithmetics.c", 16334), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_fp_arithmetics.c", 14807), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_gcm_decrypt.c", 7700), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_gcm_encrypt.c", 6928), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_hash.c", 4785), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpin.c", 7398), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpin_bad_pin.c", 7919), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpin_bad_token.c", 8210), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpin_expired_tp.c", 8192), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpin_good.c", 7889), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpin_random.c", 9633), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpin_sign.c", 13197), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpin_tp.c", 8532), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpin_vectors.c", 17027), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpinfull.c", 10703), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpinfull_onepass.c", 8951), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_mpinfull_random.c", 11622), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_octet_consistency.c", 3724), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_output_functions.c", 15843), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_rsa.c", 2961), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_rsa_sign.c", 7831), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_utils.c", 5104), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_version.c", 1095), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_wcc.c", 5893), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_wcc_gcm.c", 3163), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_wcc_random.c", 14494), ("wrappers/ios/Tests/milagro-ios-test-app/milagro-test-app/tests/test_x509.c", 29293), ("wrappers/ios/libsovrin-pod/.gitignore", 608), ("wrappers/ios/libsovrin-pod/Podfile", 1917), ("wrappers/ios/libsovrin-pod/Podfile.lock", 586), ("wrappers/ios/libsovrin-pod/libsovrin-demo.xcodeproj/project.pbxproj", 35155), ("wrappers/ios/libsovrin-pod/libsovrin-demo/AppDelegate.h", 296), ("wrappers/ios/libsovrin-pod/libsovrin-demo/AppDelegate.mm", 2090), ("wrappers/ios/libsovrin-pod/libsovrin-demo/Assets.xcassets/AppIcon.appiconset/Contents.json", 1495), ("wrappers/ios/libsovrin-pod/libsovrin-demo/Base.lproj/LaunchScreen.storyboard", 1740), ("wrappers/ios/libsovrin-pod/libsovrin-demo/Base.lproj/Main.storyboard", 1689), ("wrappers/ios/libsovrin-pod/libsovrin-demo/Info.plist", 1442), ("wrappers/ios/libsovrin-pod/libsovrin-demo/ViewController.h", 234), ("wrappers/ios/libsovrin-pod/libsovrin-demo/ViewController.m", 515), ("wrappers/ios/libsovrin-pod/libsovrin-demo/main.m", 353), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Anoncreds.m", 49009), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/AnoncredsDemo.mm", 13923), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/AnoncredsUtils.h", 3940), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/AnoncredsUtils.m", 16538), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Environment.h", 228), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Environment.m", 212), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Info.plist", 680), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Ledger.mm", 90279), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Ledger/LedgerAttribRequest.mm", 10207), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Ledger/LedgerNodeRequest.mm", 11129), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Ledger/LedgerNymRequest.mm", 23339), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Ledger/LedgerSchemaRequest.mm", 9727), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/LedgerDemo.mm", 15548), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/LedgerUtils.h", 3611), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/LedgerUtils.mm", 13262), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/NSDictionary+JSON.h", 245), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/NSDictionary+JSON.m", 1610), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Pool.mm", 2186), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/PoolUtils.h", 963), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/PoolUtils.m", 8153), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Signus/SignusHighCases.m", 35826), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Signus/SignusMediumCases.m", 23718), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/SignusDemo.mm", 7436), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/SignusUtils.h", 1610), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/SignusUtils.mm", 6146), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/TestUtils.h", 426), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/TestUtils.m", 960), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Wallet/WalletHighCases.m", 7371), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/Wallet/WalletMediumCases.m", 7672), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/WalletUtils.h", 1067), ("wrappers/ios/libsovrin-pod/libsovrin-demoTests/WalletUtils.m", 6899), ("wrappers/ios/libsovrin-pod/libsovrin.xcodeproj/project.pbxproj", 24209), ("wrappers/ios/libsovrin-pod/libsovrin/Info.plist", 753), ("wrappers/ios/libsovrin-pod/libsovrin/NSError+SovrinError.h", 204), ("wrappers/ios/libsovrin-pod/libsovrin/NSError+SovrinError.m", 333), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinAgent.h", 1617), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinAgent.mm", 4993), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinAnoncreds.h", 5008), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinAnoncreds.mm", 14121), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinCallbacks.h", 4177), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinCallbacks.mm", 22778), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinErrors.h", 2978), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinLedger.h", 4144), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinLedger.mm", 11919), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinPool.h", 927), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinPool.mm", 3709), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinSignus.h", 2304), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinSignus.mm", 6504), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinTypes.h", 68), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinWallet.h", 2286), ("wrappers/ios/libsovrin-pod/libsovrin/SovrinWallet.mm", 4658), ("wrappers/ios/libsovrin-pod/libsovrin/libsovrin.h", 763), ("wrappers/java/README.md", 7162), ("wrappers/java/lib/.gitignore", 14), ("wrappers/java/pom.xml", 2602), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/ErrorCode.java", 3447), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/LibSovrin.java", 6356), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/SovrinConstants.java", 609), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/SovrinException.java", 477), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/SovrinJava.java", 3443), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/anoncreds/Anoncreds.java", 6373), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/anoncreds/AnoncredsJSONParameters.java", 165), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/anoncreds/AnoncredsResults.java", 3606), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/ledger/Ledger.java", 10800), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/ledger/LedgerJSONParameters.java", 156), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/ledger/LedgerResults.java", 3335), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/pool/Pool.java", 4980), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/pool/PoolJSONParameters.java", 877), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/pool/PoolResults.java", 831), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/signus/Signus.java", 6514), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/signus/SignusJSONParameters.java", 616), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/signus/SignusResults.java", 1852), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/wallet/Wallet.java", 4960), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/wallet/WalletJSONParameters.java", 162), ("wrappers/java/src/main/java/org/hyperledger/indy/sdk/wallet/WalletResults.java", 811), ("wrappers/java/src/test/java/org/hyperledger/indy/sdk/LedgerTest.java", 1561), ("wrappers/java/src/test/java/org/hyperledger/indy/sdk/PoolTest.java", 1221), ("wrappers/java/src/test/java/org/hyperledger/indy/sdk/SignusTest.java", 2088), ("wrappers/java/src/test/java/org/hyperledger/indy/sdk/WalletTest.java", 1666), ("wrappers/python/sovrin/__init__.py", 205), ("wrappers/python/sovrin/anoncreds.py", 3917), ("wrappers/python/sovrin/error.py", 253), ("wrappers/python/sovrin/ledger.py", 3508), ("wrappers/python/sovrin/pool.py", 904), ("wrappers/python/sovrin/signus.py", 1828), ("wrappers/python/sovrin/wallet.py", 706), ("wrappers/python/tests/test_wallet.py", 216)].iter().map(|(p,s)|(p.to_string(), *s)).collect(),
                suggested_fix: Some(Fix::NewInclude {include: vec!["src/**/*".into(), "LICENSE".into(), "README.md".into()], has_build_script: true})
            },
            "build.rs is used but there are a bunch of extra directories that can be ignored and are not needed by the build, no manual includes/excludes"
        );
    }
    #[test]
    fn mozjs() {
        // todo: filter tests, benches, examples, image file formats, docs, allow everything in src/ , but be aware of tests/specs, implicit imports Cargo.* being explicit
        assert_eq!(
            Report::from(TaskResult::from(MOZJS)),
            Report::Version {
                total_size_in_bytes: 161225785,
                total_files: 13187,
                wasted_files: vec![],
                suggested_fix: None
            },
            "build.rs + excludes in Cargo.toml - this leaves a chance for accidental includes for which we provide an updated include list"
        );
    }
}
