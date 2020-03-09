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

fn standard_include_patterns() -> &'static [&'static str] {
    &["foo"]
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
    (
        new_include_patterns,
        added_include_patterns,
        removed_include_patterns,
    )
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

    fn standard_includes(
        _entries: Vec<TarHeader>,
        _build: Option<String>,
    ) -> (Option<Fix>, Vec<TarHeader>) {
        let _include_patterns = standard_include_patterns();
        unimplemented!("standard includes");
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
                wasted_bytes: 813680,
                wasted_files: 23,
                suggested_fix: Some(Fix::RemoveExcludeAndUseInclude {
                    include_added: ["pregenerated/aes-586-elf.S", "pregenerated/aes-586-macosx.S", "pregenerated/aes-586-win32n.obj", "pregenerated/aes-armv4-ios32.S", "pregenerated/aes-armv4-linux32.S", "pregenerated/aes-x86_64-elf.S", "pregenerated/aes-x86_64-macosx.S", "pregenerated/aes-x86_64-nasm.obj", "pregenerated/aesni-gcm-x86_64-elf.S", "pregenerated/aesni-gcm-x86_64-macosx.S", "pregenerated/aesni-gcm-x86_64-nasm.obj", "pregenerated/aesni-x86-elf.S", "pregenerated/aesni-x86-macosx.S", "pregenerated/aesni-x86-win32n.obj", "pregenerated/aesni-x86_64-elf.S", "pregenerated/aesni-x86_64-macosx.S", "pregenerated/aesni-x86_64-nasm.obj", "pregenerated/aesv8-armx-ios32.S", "pregenerated/aesv8-armx-ios64.S", "pregenerated/aesv8-armx-linux32.S", "pregenerated/aesv8-armx-linux64.S", "pregenerated/armv4-mont-ios32.S", "pregenerated/armv4-mont-linux32.S", "pregenerated/armv8-mont-ios64.S", "pregenerated/armv8-mont-linux64.S", "pregenerated/bsaes-armv7-ios32.S", "pregenerated/bsaes-armv7-linux32.S", "pregenerated/chacha-armv4-ios32.S", "pregenerated/chacha-armv4-linux32.S", "pregenerated/chacha-armv8-ios64.S", "pregenerated/chacha-armv8-linux64.S", "pregenerated/chacha-x86-elf.S", "pregenerated/chacha-x86-macosx.S", "pregenerated/chacha-x86-win32n.obj", "pregenerated/chacha-x86_64-elf.S", "pregenerated/chacha-x86_64-macosx.S", "pregenerated/chacha-x86_64-nasm.obj", "pregenerated/ecp_nistz256-armv4-ios32.S", "pregenerated/ecp_nistz256-armv4-linux32.S", "pregenerated/ecp_nistz256-armv8-ios64.S", "pregenerated/ecp_nistz256-armv8-linux64.S", "pregenerated/ecp_nistz256-x86-elf.S", "pregenerated/ecp_nistz256-x86-macosx.S", "pregenerated/ecp_nistz256-x86-win32n.obj", "pregenerated/ghash-armv4-ios32.S", "pregenerated/ghash-armv4-linux32.S", "pregenerated/ghash-x86-elf.S", "pregenerated/ghash-x86-macosx.S", "pregenerated/ghash-x86-win32n.obj", "pregenerated/ghash-x86_64-elf.S", "pregenerated/ghash-x86_64-macosx.S", "pregenerated/ghash-x86_64-nasm.obj", "pregenerated/ghashv8-armx-ios32.S", "pregenerated/ghashv8-armx-ios64.S", "pregenerated/ghashv8-armx-linux32.S", "pregenerated/ghashv8-armx-linux64.S", "pregenerated/p256-x86_64-asm-elf.S", "pregenerated/p256-x86_64-asm-macosx.S", "pregenerated/p256-x86_64-asm-nasm.obj", "pregenerated/p256_beeu-x86_64-asm-elf.S", "pregenerated/p256_beeu-x86_64-asm-macosx.S", "pregenerated/p256_beeu-x86_64-asm-nasm.obj", "pregenerated/poly1305-armv4-ios32.S", "pregenerated/poly1305-armv4-linux32.S", "pregenerated/poly1305-armv8-ios64.S", "pregenerated/poly1305-armv8-linux64.S", "pregenerated/poly1305-x86-elf.S", "pregenerated/poly1305-x86-macosx.S", "pregenerated/poly1305-x86-win32n.obj", "pregenerated/poly1305-x86_64-elf.S", "pregenerated/poly1305-x86_64-macosx.S", "pregenerated/poly1305-x86_64-nasm.obj", "pregenerated/sha256-586-elf.S", "pregenerated/sha256-586-macosx.S", "pregenerated/sha256-586-win32n.obj", "pregenerated/sha256-armv4-ios32.S", "pregenerated/sha256-armv4-linux32.S", "pregenerated/sha256-armv8-ios64.S", "pregenerated/sha256-armv8-linux64.S", "pregenerated/sha256-x86_64-elf.S", "pregenerated/sha256-x86_64-macosx.S", "pregenerated/sha256-x86_64-nasm.obj", "pregenerated/sha512-586-elf.S", "pregenerated/sha512-586-macosx.S", "pregenerated/sha512-586-win32n.obj", "pregenerated/sha512-armv4-ios32.S", "pregenerated/sha512-armv4-linux32.S", "pregenerated/sha512-armv8-ios64.S", "pregenerated/sha512-armv8-linux64.S", "pregenerated/sha512-x86_64-elf.S", "pregenerated/sha512-x86_64-macosx.S", "pregenerated/sha512-x86_64-nasm.obj", "pregenerated/vpaes-x86-elf.S", "pregenerated/vpaes-x86-macosx.S", "pregenerated/vpaes-x86-win32n.obj", "pregenerated/vpaes-x86_64-elf.S", "pregenerated/vpaes-x86_64-macosx.S", "pregenerated/vpaes-x86_64-nasm.obj", "pregenerated/x86-mont-elf.S", "pregenerated/x86-mont-macosx.S", "pregenerated/x86-mont-win32n.obj", "pregenerated/x86_64-mont-elf.S", "pregenerated/x86_64-mont-macosx.S", "pregenerated/x86_64-mont-nasm.obj", "pregenerated/x86_64-mont5-elf.S", "pregenerated/x86_64-mont5-macosx.S", "pregenerated/x86_64-mont5-nasm.obj"].iter().map(|s| s.to_string()).collect(),
                    include: ["LICENSE", "Cargo.toml", "pregenerated/aes-586-elf.S", "pregenerated/aes-586-macosx.S", "pregenerated/aes-586-win32n.obj", "pregenerated/aes-armv4-ios32.S", "pregenerated/aes-armv4-linux32.S", "pregenerated/aes-x86_64-elf.S", "pregenerated/aes-x86_64-macosx.S", "pregenerated/aes-x86_64-nasm.obj", "pregenerated/aesni-gcm-x86_64-elf.S", "pregenerated/aesni-gcm-x86_64-macosx.S", "pregenerated/aesni-gcm-x86_64-nasm.obj", "pregenerated/aesni-x86-elf.S", "pregenerated/aesni-x86-macosx.S", "pregenerated/aesni-x86-win32n.obj", "pregenerated/aesni-x86_64-elf.S", "pregenerated/aesni-x86_64-macosx.S", "pregenerated/aesni-x86_64-nasm.obj", "pregenerated/aesv8-armx-ios32.S", "pregenerated/aesv8-armx-ios64.S", "pregenerated/aesv8-armx-linux32.S", "pregenerated/aesv8-armx-linux64.S", "pregenerated/armv4-mont-ios32.S", "pregenerated/armv4-mont-linux32.S", "pregenerated/armv8-mont-ios64.S", "pregenerated/armv8-mont-linux64.S", "pregenerated/bsaes-armv7-ios32.S", "pregenerated/bsaes-armv7-linux32.S", "pregenerated/chacha-armv4-ios32.S", "pregenerated/chacha-armv4-linux32.S", "pregenerated/chacha-armv8-ios64.S", "pregenerated/chacha-armv8-linux64.S", "pregenerated/chacha-x86-elf.S", "pregenerated/chacha-x86-macosx.S", "pregenerated/chacha-x86-win32n.obj", "pregenerated/chacha-x86_64-elf.S", "pregenerated/chacha-x86_64-macosx.S", "pregenerated/chacha-x86_64-nasm.obj", "pregenerated/ecp_nistz256-armv4-ios32.S", "pregenerated/ecp_nistz256-armv4-linux32.S", "pregenerated/ecp_nistz256-armv8-ios64.S", "pregenerated/ecp_nistz256-armv8-linux64.S", "pregenerated/ecp_nistz256-x86-elf.S", "pregenerated/ecp_nistz256-x86-macosx.S", "pregenerated/ecp_nistz256-x86-win32n.obj", "pregenerated/ghash-armv4-ios32.S", "pregenerated/ghash-armv4-linux32.S", "pregenerated/ghash-x86-elf.S", "pregenerated/ghash-x86-macosx.S", "pregenerated/ghash-x86-win32n.obj", "pregenerated/ghash-x86_64-elf.S", "pregenerated/ghash-x86_64-macosx.S", "pregenerated/ghash-x86_64-nasm.obj", "pregenerated/ghashv8-armx-ios32.S", "pregenerated/ghashv8-armx-ios64.S", "pregenerated/ghashv8-armx-linux32.S", "pregenerated/ghashv8-armx-linux64.S", "pregenerated/p256-x86_64-asm-elf.S", "pregenerated/p256-x86_64-asm-macosx.S", "pregenerated/p256-x86_64-asm-nasm.obj", "pregenerated/p256_beeu-x86_64-asm-elf.S", "pregenerated/p256_beeu-x86_64-asm-macosx.S", "pregenerated/p256_beeu-x86_64-asm-nasm.obj", "pregenerated/poly1305-armv4-ios32.S", "pregenerated/poly1305-armv4-linux32.S", "pregenerated/poly1305-armv8-ios64.S", "pregenerated/poly1305-armv8-linux64.S", "pregenerated/poly1305-x86-elf.S", "pregenerated/poly1305-x86-macosx.S", "pregenerated/poly1305-x86-win32n.obj", "pregenerated/poly1305-x86_64-elf.S", "pregenerated/poly1305-x86_64-macosx.S", "pregenerated/poly1305-x86_64-nasm.obj", "pregenerated/sha256-586-elf.S", "pregenerated/sha256-586-macosx.S", "pregenerated/sha256-586-win32n.obj", "pregenerated/sha256-armv4-ios32.S", "pregenerated/sha256-armv4-linux32.S", "pregenerated/sha256-armv8-ios64.S", "pregenerated/sha256-armv8-linux64.S", "pregenerated/sha256-x86_64-elf.S", "pregenerated/sha256-x86_64-macosx.S", "pregenerated/sha256-x86_64-nasm.obj", "pregenerated/sha512-586-elf.S", "pregenerated/sha512-586-macosx.S", "pregenerated/sha512-586-win32n.obj", "pregenerated/sha512-armv4-ios32.S", "pregenerated/sha512-armv4-linux32.S", "pregenerated/sha512-armv8-ios64.S", "pregenerated/sha512-armv8-linux64.S", "pregenerated/sha512-x86_64-elf.S", "pregenerated/sha512-x86_64-macosx.S", "pregenerated/sha512-x86_64-nasm.obj", "pregenerated/vpaes-x86-elf.S", "pregenerated/vpaes-x86-macosx.S", "pregenerated/vpaes-x86-win32n.obj", "pregenerated/vpaes-x86_64-elf.S", "pregenerated/vpaes-x86_64-macosx.S", "pregenerated/vpaes-x86_64-nasm.obj", "pregenerated/x86-mont-elf.S", "pregenerated/x86-mont-macosx.S", "pregenerated/x86-mont-win32n.obj", "pregenerated/x86_64-mont-elf.S", "pregenerated/x86_64-mont-macosx.S", "pregenerated/x86_64-mont-nasm.obj", "pregenerated/x86_64-mont5-elf.S", "pregenerated/x86_64-mont5-macosx.S", "pregenerated/x86_64-mont5-nasm.obj", "build.rs", "crypto/block.c", "crypto/block.h", "crypto/chacha/asm/chacha-armv4.pl", "crypto/chacha/asm/chacha-armv8.pl", "crypto/chacha/asm/chacha-x86.pl", "crypto/chacha/asm/chacha-x86_64.pl", "crypto/cipher_extra/asm/aes128gcmsiv-x86_64.pl", "crypto/cipher_extra/test/aes_128_gcm_siv_tests.txt", "crypto/cipher_extra/test/aes_256_gcm_siv_tests.txt", "crypto/constant_time_test.c", "crypto/cpu-aarch64-linux.c", "crypto/cpu-arm-linux.c", "crypto/cpu-arm.c", "crypto/cpu-intel.c", "crypto/crypto.c", "crypto/curve25519/asm/x25519-asm-arm.S", "crypto/fipsmodule/aes/aes.c", "crypto/fipsmodule/aes/asm/aes-586.pl", "crypto/fipsmodule/aes/asm/aes-armv4.pl", "crypto/fipsmodule/aes/asm/aes-x86_64.pl", "crypto/fipsmodule/aes/asm/aesni-x86.pl", "crypto/fipsmodule/aes/asm/aesni-x86_64.pl", "crypto/fipsmodule/aes/asm/aesv8-armx.pl", "crypto/fipsmodule/aes/asm/bsaes-armv7.pl", "crypto/fipsmodule/aes/asm/bsaes-x86_64.pl", "crypto/fipsmodule/aes/asm/vpaes-x86.pl", "crypto/fipsmodule/aes/asm/vpaes-x86_64.pl", "crypto/fipsmodule/aes/internal.h", "crypto/fipsmodule/bn/asm/armv4-mont.pl", "crypto/fipsmodule/bn/asm/armv8-mont.pl", "crypto/fipsmodule/bn/asm/x86-mont.pl", "crypto/fipsmodule/bn/asm/x86_64-mont.pl", "crypto/fipsmodule/bn/asm/x86_64-mont5.pl", "crypto/fipsmodule/bn/generic.c", "crypto/fipsmodule/bn/internal.h", "crypto/fipsmodule/bn/montgomery.c", "crypto/fipsmodule/bn/montgomery_inv.c", "crypto/fipsmodule/cipher/e_aes.c", "crypto/fipsmodule/ec/asm/ecp_nistz256-armv4.pl", "crypto/fipsmodule/ec/asm/ecp_nistz256-armv8.pl", "crypto/fipsmodule/ec/asm/ecp_nistz256-x86.pl", "crypto/fipsmodule/ec/asm/p256-x86_64-asm.pl", "crypto/fipsmodule/ec/ecp_nistz.c", "crypto/fipsmodule/ec/ecp_nistz.h", "crypto/fipsmodule/ec/ecp_nistz256.c", "crypto/fipsmodule/ec/ecp_nistz256.h", "crypto/fipsmodule/ec/ecp_nistz256_table.inl", "crypto/fipsmodule/ec/ecp_nistz384.h", "crypto/fipsmodule/ec/ecp_nistz384.inl", "crypto/fipsmodule/ec/gfp_p256.c", "crypto/fipsmodule/ec/gfp_p384.c", "crypto/fipsmodule/ecdsa/ecdsa_verify_tests.txt", "crypto/fipsmodule/modes/asm/aesni-gcm-x86_64.pl", "crypto/fipsmodule/modes/asm/ghash-armv4.pl", "crypto/fipsmodule/modes/asm/ghash-x86.pl", "crypto/fipsmodule/modes/asm/ghash-x86_64.pl", "crypto/fipsmodule/modes/asm/ghashv8-armx.pl", "crypto/fipsmodule/modes/gcm.c", "crypto/fipsmodule/modes/internal.h", "crypto/fipsmodule/sha/asm/sha256-586.pl", "crypto/fipsmodule/sha/asm/sha256-armv4.pl", "crypto/fipsmodule/sha/asm/sha512-586.pl", "crypto/fipsmodule/sha/asm/sha512-armv4.pl", "crypto/fipsmodule/sha/asm/sha512-armv8.pl", "crypto/fipsmodule/sha/asm/sha512-x86_64.pl", "crypto/internal.h", "crypto/limbs/limbs.c", "crypto/limbs/limbs.h", "crypto/limbs/limbs.inl", "crypto/mem.c", "crypto/perlasm/arm-xlate.pl", "crypto/perlasm/x86asm.pl", "crypto/perlasm/x86gas.pl", "crypto/perlasm/x86nasm.pl", "crypto/perlasm/x86_64-xlate.pl", "crypto/poly1305/asm/poly1305-armv4.pl", "crypto/poly1305/asm/poly1305-armv8.pl", "crypto/poly1305/asm/poly1305-x86.pl", "crypto/poly1305/asm/poly1305-x86_64.pl", "examples/checkdigest.rs", "include/GFp/aes.h", "include/GFp/arm_arch.h", "include/GFp/base.h", "include/GFp/cpu.h", "include/GFp/mem.h", "include/GFp/type_check.h", "src/aead.rs", "src/aead/aes.rs", "src/aead/aes_gcm.rs", "src/aead/aes_tests.txt", "src/aead/block.rs", "src/aead/chacha.rs", "src/aead/chacha_tests.txt", "src/aead/chacha20_poly1305.rs", "src/aead/chacha20_poly1305_openssh.rs", "src/aead/gcm.rs", "src/aead/nonce.rs", "src/aead/poly1305.rs", "src/aead/poly1305_test.txt", "src/aead/shift.rs", "src/agreement.rs", "src/arithmetic.rs", "src/arithmetic/montgomery.rs", "src/array.rs", "src/bits.rs", "src/bssl.rs", "src/c.rs", "src/constant_time.rs", "src/cpu.rs", "src/data/alg-rsa-encryption.der", "src/debug.rs", "src/digest.rs", "src/digest/sha1.rs", "src/ec/curve25519/ed25519/digest.rs", "src/ec/curve25519/ed25519.rs", "src/ec/curve25519/ed25519/signing.rs", "src/ec/curve25519/ed25519/verification.rs", "src/ec/curve25519/ed25519/ed25519_pkcs8_v2_template.der", "src/ec/curve25519.rs", "src/ec/curve25519/ops.rs", "src/ec/curve25519/x25519.rs", "src/ec.rs", "src/ec/keys.rs", "src/ec/suite_b/curve.rs", "src/ec/suite_b/ecdh.rs", "src/ec/suite_b/ecdsa/digest_scalar.rs", "src/ec/suite_b/ecdsa.rs", "src/ec/suite_b/ecdsa/signing.rs", "src/ec/suite_b/ecdsa/verification.rs", "src/ec/suite_b/ecdsa/ecdsa_digest_scalar_tests.txt", "src/ec/suite_b/ecdsa/ecPublicKey_p256_pkcs8_v1_template.der", "src/ec/suite_b/ecdsa/ecPublicKey_p384_pkcs8_v1_template.der", "src/ec/suite_b/ecdsa/ecdsa_sign_asn1_tests.txt", "src/ec/suite_b/ecdsa/ecdsa_sign_fixed_tests.txt", "src/ec/suite_b.rs", "src/ec/suite_b/ops/elem.rs", "src/ec/suite_b/ops.rs", "src/ec/suite_b/ops/p256.rs", "src/ec/suite_b/ops/p256_elem_mul_tests.txt", "src/ec/suite_b/ops/p256_elem_neg_tests.txt", "src/ec/suite_b/ops/p256_elem_sum_tests.txt", "src/ec/suite_b/ops/p256_point_double_tests.txt", "src/ec/suite_b/ops/p256_point_mul_base_tests.txt", "src/ec/suite_b/ops/p256_point_mul_serialized_tests.txt", "src/ec/suite_b/ops/p256_point_mul_tests.txt", "src/ec/suite_b/ops/p256_point_sum_mixed_tests.txt", "src/ec/suite_b/ops/p256_point_sum_tests.txt", "src/ec/suite_b/ops/p256_scalar_mul_tests.txt", "src/ec/suite_b/ops/p256_scalar_square_tests.txt", "src/ec/suite_b/ops/p384.rs", "src/ec/suite_b/ops/p384_elem_div_by_2_tests.txt", "src/ec/suite_b/ops/p384_elem_mul_tests.txt", "src/ec/suite_b/ops/p384_elem_neg_tests.txt", "src/ec/suite_b/ops/p384_elem_sum_tests.txt", "src/ec/suite_b/ops/p384_point_double_tests.txt", "src/ec/suite_b/ops/p384_point_mul_base_tests.txt", "src/ec/suite_b/ops/p384_point_mul_tests.txt", "src/ec/suite_b/ops/p384_point_sum_tests.txt", "src/ec/suite_b/ops/p384_scalar_mul_tests.txt", "src/ec/suite_b/private_key.rs", "src/ec/suite_b/public_key.rs", "src/ec/suite_b/suite_b_public_key_tests.txt", "src/endian.rs", "src/error.rs", "src/hkdf.rs", "src/hmac.rs", "src/hmac_generate_serializable_tests.txt", "src/io.rs", "src/io/der.rs", "src/io/der_writer.rs", "src/io/writer.rs", "src/lib.rs", "src/limb.rs", "src/endian.rs", "src/pbkdf2.rs", "src/pkcs8.rs", "src/polyfill.rs", "src/polyfill/convert.rs", "src/rand.rs", "src/rsa/bigint.rs", "src/rsa/bigint_elem_exp_consttime_tests.txt", "src/rsa/bigint_elem_exp_vartime_tests.txt", "src/rsa/bigint_elem_mul_tests.txt", "src/rsa/bigint_elem_reduced_once_tests.txt", "src/rsa/bigint_elem_reduced_tests.txt", "src/rsa/bigint_elem_squared_tests.txt", "src/rsa/convert_nist_rsa_test_vectors.py", "src/rsa.rs", "src/rsa/padding.rs", "src/rsa/random.rs", "src/rsa/rsa_pss_padding_tests.txt", "src/rsa/signature_rsa_example_private_key.der", "src/rsa/signature_rsa_example_public_key.der", "src/rsa/signing.rs", "src/rsa/verification.rs", "src/signature.rs", "src/test.rs", "src/test_1_syntax_error_tests.txt", "src/test_1_tests.txt", "src/test_3_tests.txt", "tests/aead_aes_128_gcm_tests.txt", "tests/aead_aes_256_gcm_tests.txt", "tests/aead_chacha20_poly1305_tests.txt", "tests/aead_chacha20_poly1305_openssh_tests.txt", "tests/aead_tests.rs", "tests/agreement_tests.rs", "tests/agreement_tests.txt", "tests/digest_tests.rs", "tests/digest_tests.txt", "tests/ecdsa_from_pkcs8_tests.txt", "tests/ecdsa_tests.rs", "tests/ecdsa_sign_asn1_tests.txt", "tests/ecdsa_sign_fixed_tests.txt", "tests/ecdsa_verify_asn1_tests.txt", "tests/ecdsa_verify_fixed_tests.txt", "tests/ed25519_from_pkcs8_tests.txt", "tests/ed25519_from_pkcs8_unchecked_tests.txt", "tests/ed25519_tests.rs", "tests/ed25519_tests.txt", "tests/ed25519_test_private_key.bin", "tests/ed25519_test_public_key.bin", "tests/hkdf_tests.rs", "tests/hkdf_tests.txt", "tests/hmac_tests.rs", "tests/hmac_tests.txt", "tests/pbkdf2_tests.rs", "tests/pbkdf2_tests.txt", "tests/rsa_from_pkcs8_tests.txt", "tests/rsa_pkcs1_sign_tests.txt", "tests/rsa_pkcs1_verify_tests.txt", "tests/rsa_primitive_verify_tests.txt", "tests/rsa_pss_sign_tests.txt", "tests/rsa_pss_verify_tests.txt", "tests/rsa_tests.rs", "tests/signature_tests.rs", "third_party/fiat/curve25519.c", "third_party/fiat/curve25519_tables.h", "third_party/fiat/internal.h", "third_party/fiat/LICENSE", "third_party/fiat/make_curve25519_tables.py", "third_party/NIST/SHAVS/SHA1LongMsg.rsp", "third_party/NIST/SHAVS/SHA1Monte.rsp", "third_party/NIST/SHAVS/SHA1ShortMsg.rsp", "third_party/NIST/SHAVS/SHA224LongMsg.rsp", "third_party/NIST/SHAVS/SHA224Monte.rsp", "third_party/NIST/SHAVS/SHA224ShortMsg.rsp", "third_party/NIST/SHAVS/SHA256LongMsg.rsp", "third_party/NIST/SHAVS/SHA256Monte.rsp", "third_party/NIST/SHAVS/SHA256ShortMsg.rsp", "third_party/NIST/SHAVS/SHA384LongMsg.rsp", "third_party/NIST/SHAVS/SHA384Monte.rsp", "third_party/NIST/SHAVS/SHA384ShortMsg.rsp", "third_party/NIST/SHAVS/SHA512LongMsg.rsp", "third_party/NIST/SHAVS/SHA512Monte.rsp", "third_party/NIST/SHAVS/SHA512ShortMsg.rsp"].iter().map(|s|s.to_string()).collect(),
                    include_removed: vec!["pregenerated/*".into()]
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
                suggested_fix: Some(Fix::NewInclude {include: vec![], has_build_script: true})
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
            "build.rs + excludes in Cargo.toml - this leaves a chance for accidental includes for which we provide an updated include list"
        );
    }
}
