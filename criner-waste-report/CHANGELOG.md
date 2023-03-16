# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

A maintenance release without user-facing changes.

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 11 commits contributed to the release over the course of 820 calendar days.
 - 941 days passed between releases.
 - 0 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Thanks Clippy

<csr-read-only-do-not-edit/>

[Clippy](https://github.com/rust-lang/rust-clippy) helped 3 times to make code idiomatic. 

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Upgrade `toml` in `criner-waste-report` ([`7be638b`](https://github.com/the-lean-crate/criner/commit/7be638bab4c6c7d7c2f753470d09d77ac9bc5ed2))
    - Upgrade dia-semver ([`2e3ab36`](https://github.com/the-lean-crate/criner/commit/2e3ab36a2360ecbf50abfae20c6a25ba7889ca52))
    - Thanks clippy ([`459cc26`](https://github.com/the-lean-crate/criner/commit/459cc26ef2bf0da1c74c807dc355db7ac3497a6a))
    - Upgrade to rmp-serde 1.0 ([`b6b1109`](https://github.com/the-lean-crate/criner/commit/b6b1109e8feb220bdc9ddd834182cb2734a1394f))
    - Update changelogs with `cargo changelog` ([`e80897e`](https://github.com/the-lean-crate/criner/commit/e80897e265ab4d5af7e095a106516bc701c3f315))
    - Cleanup changelogs ([`5553dc2`](https://github.com/the-lean-crate/criner/commit/5553dc208f0463e02a25f7250a71c1c144c2f330))
    - Thanks clippy ([`07c6594`](https://github.com/the-lean-crate/criner/commit/07c659410f252631f982dda39b4003f3c75da33c))
    - Dependency upgrade ([`2f8c330`](https://github.com/the-lean-crate/criner/commit/2f8c3308dbbc28792471a24fbd0d0e544875de4b))
    - Thanks clippy ([`b4fb778`](https://github.com/the-lean-crate/criner/commit/b4fb7783d67f9605ff0f97d299e075a2df3bc5fb))
    - Dependency upgrade ([`c583f50`](https://github.com/the-lean-crate/criner/commit/c583f50ff3e8db1f81309778d06980cae5047fb5))
    - Cargo clippy ([`d285e06`](https://github.com/the-lean-crate/criner/commit/d285e0609eb699bfb164d584ca44a99dbe2c8d71))
</details>

## v0.1.4 (2020-07-25)

* fix https://github.com/the-lean-crate/cargo-diet/issues/6

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release over the course of 14 calendar days.
 - 57 days passed between releases.
 - 0 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Use more generous globs for exclude patterns ([`4cd591d`](https://github.com/the-lean-crate/criner/commit/4cd591d1dc0fd00bda2f632558dd73e230301c0f))
    - Allow for more screenspace via rustfmt config file ([`50dcbac`](https://github.com/the-lean-crate/criner/commit/50dcbac5a4c629dbd292c5b57e222a171299d985))
</details>

## v0.1.3 (2020-05-28)

* back to the state of 0.1.1 - serde is actually required

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 0 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Revert previous change ([`5b6c614`](https://github.com/the-lean-crate/criner/commit/5b6c61445df49aa8ad545fb591c3f9fc7b7cd452))
    - Revert "serde is now behind a feature toggle for criner-waste-report" ([`73c38a0`](https://github.com/the-lean-crate/criner/commit/73c38a0698983a24e1c14db8979c9ed5efd232d8))
</details>

## v0.1.2 (2020-05-28)

* serde serialization and deserialization capabilities are behind the feature flag 'with-serde', which is enabled by default.

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 3 commits contributed to the release over the course of 2 calendar days.
 - 4 days passed between releases.
 - 0 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Bump patch level of criner-waste-report ([`90f5930`](https://github.com/the-lean-crate/criner/commit/90f5930c80825eed7574c0fa7cba9039c95f5687))
    - Serde is now behind a feature toggle for criner-waste-report ([`821a15a`](https://github.com/the-lean-crate/criner/commit/821a15a8231597fb99851849ff1740071107e4a9))
    - Update all + cargo diet ([`aa1a31e`](https://github.com/the-lean-crate/criner/commit/aa1a31e0ddea775f1c189645af0bf09ce8fa44b5))
</details>

## v0.1.1 (2020-05-24)

* remove tar depdendency

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 0 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Bump patch level ([`7bfdaa5`](https://github.com/the-lean-crate/criner/commit/7bfdaa582633f15e30316b78836ae21224594ecd))
    - Remove unnecessary tar dependency in criner-waste-report… ([`844512f`](https://github.com/the-lean-crate/criner/commit/844512ff10b678ffd750c24e066b2b246354aa88))
</details>

## v0.1.0 (2020-05-24)

* initial release

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 5 commits contributed to the release.
 - 0 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Prepare release of criner-waste-report ([`ddac38b`](https://github.com/the-lean-crate/criner/commit/ddac38bd31ccdfb88b18370fac8d5c40c8c39a9c))
    - Refactor ([`a794d02`](https://github.com/the-lean-crate/criner/commit/a794d020e2d403379edd5956666bd8113266cc1d))
    - Split html related criner-waste-report crates into their own feature ([`a9a3a19`](https://github.com/the-lean-crate/criner/commit/a9a3a194cf05cf8088a045a13ad4c6e5f2a494b0))
    - Organize dependencies before splitting html out as feature ([`d8d336a`](https://github.com/the-lean-crate/criner/commit/d8d336a4180b6f800567d057c4a3b1c32d546b35))
    - Make use of new criner-waste-report crate within criner ([`acc520e`](https://github.com/the-lean-crate/criner/commit/acc520e065f4969024bf0ce4d5d4e5acb5bd8b33))
</details>

