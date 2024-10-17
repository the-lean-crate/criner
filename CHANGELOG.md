# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.4.0 (2024-10-17)

### Chore (BREAKING)

 - <csr-id-081cc14b90e4718ef45190cff1239a9ff5f9a1e7/> upgrade gix-related dependencies

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 2 commits contributed to the release.
 - 1 commit was understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Merge branch 'upgrades' ([`cf7fe54`](https://github.com/the-lean-crate/criner/commit/cf7fe541d7a40c21f06c1e256d8f1072439c27d9))
    - Upgrade gix-related dependencies ([`081cc14`](https://github.com/the-lean-crate/criner/commit/081cc14b90e4718ef45190cff1239a9ff5f9a1e7))
</details>

## 0.3.1 (2023-03-16)

A maintenance release without user-facing changes.

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 10 commits contributed to the release.
 - 0 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Upgrade `git2`, `crates-index-diff` and `prodash`. ([`09fb11f`](https://github.com/the-lean-crate/criner/commit/09fb11f4077b8426aadc139fe8d72dfdf6d65bbe))
    - Upgrade clap ([`f54286f`](https://github.com/the-lean-crate/criner/commit/f54286f7b76ac8f5daf5d4d13670347ce79fbe08))
    - Merge branch 'upgrade-index-diff' ([`85e0ca1`](https://github.com/the-lean-crate/criner/commit/85e0ca1b4c9e8abefc450fca89e1f6d8b5c9d17e))
    - Add a flag to skip downloading the database entirely ([`c8908bf`](https://github.com/the-lean-crate/criner/commit/c8908bf5356626e4cfd0f0a7ddd24cd9b6f96e09))
    - Fix deprectation warnings ([`2af218b`](https://github.com/the-lean-crate/criner/commit/2af218bb173f9887151f33b3d8395df6e1cddd94))
    - Fix all of the time::format descriptions to v0.3 ([`25a6416`](https://github.com/the-lean-crate/criner/commit/25a64167c340b61a8f25db79293f910bf452b744))
    - Upgrade to latest time/prodash at the loss of local time support ([`1100c83`](https://github.com/the-lean-crate/criner/commit/1100c830a8a9bf21c60d8e65f19953e71fa752ef))
    - Upgrade to latest clap ([`9302abc`](https://github.com/the-lean-crate/criner/commit/9302abc18056fe249f42bcdd006970543c7ecb12))
    - Dependency upgrade ([`6089587`](https://github.com/the-lean-crate/criner/commit/6089587fd23645ba16590eb639cbcd9eae7228d1))
    - Cargo clippy ([`d285e06`](https://github.com/the-lean-crate/criner/commit/d285e0609eb699bfb164d584ca44a99dbe2c8d71))
</details>

## v0.3.0 (2020-11-02)

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 5 commits contributed to the release over the course of 115 calendar days.
 - 139 days passed between releases.
 - 0 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Fix build ([`257a601`](https://github.com/the-lean-crate/criner/commit/257a60192d6543648e6684b07a40024b8f894957))
    - Upgrade to prodash 10 ([`72cccf7`](https://github.com/the-lean-crate/criner/commit/72cccf77a5e228fdbbe7ee60c75f1db5f3ad1a37))
    - Replace structopt with Clap 3 ([`c2313b3`](https://github.com/the-lean-crate/criner/commit/c2313b3601e8a848ae68f42301a3f113bdd807af))
    - Allow for more screenspace via rustfmt config file ([`50dcbac`](https://github.com/the-lean-crate/criner/commit/50dcbac5a4c629dbd292c5b57e222a171299d985))
    - Upgrade to prodash 7.0 ([`83d8029`](https://github.com/the-lean-crate/criner/commit/83d8029d782e7d3a6780f66d7383c83c95df3c26))
</details>

## v0.2.0 (2020-06-16)

## v0.1.4 (2020-07-25)

## v0.1.3 (2020-05-28)

## v0.1.2 (2020-04-12)

* the first release of criner-cli. Early, but able to get you started on your personal crates.io download.

### Commit Statistics

<csr-read-only-do-not-edit/>

 - 29 commits contributed to the release over the course of 50 calendar days.
 - 0 commits were understood as [conventional](https://www.conventionalcommits.org).
 - 0 issues like '(#ID)' were seen in commit messages

### Commit Details

<csr-read-only-do-not-edit/>

<details><summary>view details</summary>

 * **Uncategorized**
    - Add default value for db-path ([`dbffa6b`](https://github.com/the-lean-crate/criner/commit/dbffa6bf807e879b67bfbf3f1fbf396a0f60ba88))
    - More efficient drawing on idle, putting CPU usage to half or a third. ([`5b34d88`](https://github.com/the-lean-crate/criner/commit/5b34d88fad62cbf58cecf713374579dcfb047ac3))
    - Very first sketch on how to schedule something every 24h ([`6046420`](https://github.com/the-lean-crate/criner/commit/604642096b84ebcb2d7bb600fce054795179aa3e))
    - More stable gui experience ([`a798b1f`](https://github.com/the-lean-crate/criner/commit/a798b1fb46c3e4d5d32c5207543d42d9f37ca782))
    - Don't write 'yanked …' message, it's log spamming ([`bc2cff6`](https://github.com/the-lean-crate/criner/commit/bc2cff6f0e5c78c2b383a9bcf79e847224cf0008))
    - Make aliases more obvious, increase scrollback buffer size ([`2fb5fb1`](https://github.com/the-lean-crate/criner/commit/2fb5fb120aa569f798bf2f4cb938114fa98021c1))
    - Don't create commits if there was no change, save unnecessary history ([`d7b9c61`](https://github.com/the-lean-crate/criner/commit/d7b9c61cb2278cc0e866cf152a5c8f1781532adf))
    - Some more FPS by default, we can afford it ([`abaeb61`](https://github.com/the-lean-crate/criner/commit/abaeb617b6965c6200fd43368747d7dc45afe2fe))
    - Always initialize an env-logger for non-gui subcommands ([`0898d52`](https://github.com/the-lean-crate/criner/commit/0898d52affcf470807df7d86110d5f030f46b46a))
    - Separate processing and reporting stage, which works due to avoiding… ([`e871dfb`](https://github.com/the-lean-crate/criner/commit/e871dfbbf8326a71b1cebcd51db63db2c81073a5))
    - Since we cannot spawn futures with statements, bundle… ([`c40aa25`](https://github.com/the-lean-crate/criner/commit/c40aa25dab665188094dac24a5b645191d0d9be5))
    - Add support for globbing to limit runtime of reporting ([`79bd2e3`](https://github.com/the-lean-crate/criner/commit/79bd2e31d0e01d67943b6e71253cbe89411ec789))
    - Allow to run the reporting stage separately, to allow turning it off ([`0841822`](https://github.com/the-lean-crate/criner/commit/0841822d6e3e405e96b5a1a47dcc687191ee8e8b))
    - Allow passing options on how often to run stages to CLI ([`e6ad22e`](https://github.com/the-lean-crate/criner/commit/e6ad22ee98305e3bea5c04fc16ca8511f4875060))
    - Automatically turn on the logger in no-gui, but allow people to override it ([`b5e74b6`](https://github.com/the-lean-crate/criner/commit/b5e74b61fd2cd2301741117a43d8cd7fa292880b))
    - First part of exporting crate versions ([`ee2dfa5`](https://github.com/the-lean-crate/criner/commit/ee2dfa5539ee455a1fce43a4ca4f0fa84004005c))
    - Frame for exporting an sqlite database into a clearer form ([`0394e86`](https://github.com/the-lean-crate/criner/commit/0394e86193904018ef082d7e06e895607c6b7c1f))
    - Control intervals from the command-line ([`d478bc5`](https://github.com/the-lean-crate/criner/commit/d478bc5f539a19632aaccee6d1218e4e653fe10c))
    - Spawn cpu+output  bound processors (for now dummy ones) ([`896de2b`](https://github.com/the-lean-crate/criner/commit/896de2b1e52de55beaf73107b92dbea509715d78))
    - Fix args ([`2d9bea9`](https://github.com/the-lean-crate/criner/commit/2d9bea983b5baffa6f34b261af66106919f3c4d2))
    - Prepare for CPU bound processors ([`a928d27`](https://github.com/the-lean-crate/criner/commit/a928d274eeb72003de43daf5cf54b041ab438ecd))
    - Let processing stage handle its own workers ([`d8d640d`](https://github.com/the-lean-crate/criner/commit/d8d640ddd3ebf6cd264f86d5fd3d2b8ac4ad944d))
    - Extract engine runner ([`cdd2c0e`](https://github.com/the-lean-crate/criner/commit/cdd2c0ee03d81e6e09c52ffe191b59bd8ba33c79))
    - First migration ([`fd30e97`](https://github.com/the-lean-crate/criner/commit/fd30e97e55dd37b7e8e6e9ae979d56ac6cbfadbd))
    - Initial version of migration command ([`b149866`](https://github.com/the-lean-crate/criner/commit/b1498662841844b451c3240f340224d35116d9f9))
    - Store downloads only in assets directory, now part of the DB ([`dc4d7aa`](https://github.com/the-lean-crate/criner/commit/dc4d7aa59539d4c0c23cfa80624061685916f392))
    - First rough CLI startup ([`cfb6eb5`](https://github.com/the-lean-crate/criner/commit/cfb6eb53c80cc31a5664bb640314b42eac547315))
    - Prepare criner-only CLI ([`4d5a235`](https://github.com/the-lean-crate/criner/commit/4d5a2354b90ea9f243cae8a248f2ca8fcc36dc44))
    - Initial commit as copy from crates-io-cli ([`2dfefdf`](https://github.com/the-lean-crate/criner/commit/2dfefdf902c1bea243489f9deebce95c8bc8b4ac))
</details>

## v0.1.1 (2020-03-20)

## v0.1.0 (2020-03-20)

