use serde_derive::{Deserialize, Serialize};
use std::{
    borrow::Cow, collections::HashMap, iter::FromIterator, ops::Add, time::Duration,
    time::SystemTime,
};

/// Represents a top-level crate and associated information
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Crate<'a> {
    /// All versions published to crates.io, guaranteed to be sorted so that the most recent version is last.
    /// The format is as specified in Cargo.toml:version
    pub versions: Vec<Cow<'a, str>>,
}

impl<'a> From<&crates_index_diff::CrateVersion> for Crate<'a> {
    fn from(v: &crates_index_diff::CrateVersion) -> Self {
        Crate {
            versions: vec![v.version.to_owned().into()],
        }
    }
}

/// Stores element counts of various kinds
#[derive(Default, Debug, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Clone)]
pub struct Counts {
    /// The amount of crate versions stored in the database
    pub crate_versions: u64,

    /// The amount of crates in the database
    pub crates: u32,
}

/// Stores wall clock time that elapsed for various kinds of computation
#[derive(Default, Debug, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Clone)]
pub struct Durations {
    pub fetch_crate_versions: Duration,
}

/// Stores information about the work we have performed thus far
#[derive(Default, Debug, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Clone)]
pub struct Context {
    /// Various elements counts
    pub counts: Counts,
    /// Various kinds of time we took for computation
    pub durations: Durations,
}

impl Add<&Context> for Context {
    type Output = Context;

    fn add(self, rhs: &Context) -> Self::Output {
        Context {
            counts: Counts {
                crate_versions: self.counts.crate_versions + rhs.counts.crate_versions,
                crates: self.counts.crates + rhs.counts.crates,
            },
            durations: Durations {
                fetch_crate_versions: self.durations.fetch_crate_versions
                    + rhs.durations.fetch_crate_versions,
            },
        }
    }
}

/// A single dependency of a specific crate version
#[derive(Clone, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Debug)]
pub struct Dependency<'a> {
    /// The crate name
    pub name: Cow<'a, str>,
    /// The version the parent crate requires of this dependency
    #[serde(rename = "req")]
    pub required_version: Cow<'a, str>,
    /// All cargo features configured by the parent crate
    pub features: Vec<Cow<'a, str>>,
    /// True if this is an optional dependency
    pub optional: bool,
    /// True if default features are enabled
    pub default_features: bool,
    /// The name of the build target
    pub target: Option<Cow<'a, str>>,
    /// The kind of dependency, usually 'normal' or 'dev'
    pub kind: Option<Cow<'a, str>>,
    /// The package this crate is contained in
    pub package: Option<Cow<'a, str>>,
}

impl<'a> From<&crates_index_diff::Dependency> for Dependency<'a> {
    fn from(v: &crates_index_diff::Dependency) -> Self {
        Dependency {
            name: v.name.to_owned().into(),
            required_version: v.required_version.to_owned().into(),
            features: v
                .features
                .iter()
                .map(ToOwned::to_owned)
                .map(Into::into)
                .collect(),
            optional: v.optional,
            default_features: v.default_features,
            target: v.target.as_ref().map(|v| v.to_owned().into()),
            kind: v.kind.as_ref().map(|v| v.to_owned().into()),
            package: v.package.as_ref().map(|v| v.to_owned().into()),
        }
    }
}

/// Pack all information we know about a change made to a version of a crate.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct CrateVersion<'a> {
    /// The crate name, i.e. `clap`.
    pub name: Cow<'a, str>,
    /// The kind of change.
    #[serde(rename = "yanked")]
    pub kind: crates_index_diff::ChangeKind,
    /// The semantic version of the crate.
    #[serde(rename = "vers")]
    pub version: Cow<'a, str>,
    /// The checksum over the crate archive
    #[serde(rename = "cksum")]
    pub checksum: Cow<'a, str>,
    /// All cargo features
    pub features: HashMap<Cow<'a, str>, Vec<Cow<'a, str>>>,
    /// All crate dependencies
    #[serde(rename = "deps")]
    pub dependencies: Vec<Dependency<'a>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ReportResult {
    Done,
    NotStarted,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TaskState {
    /// The task was never started
    NotStarted,
    /// The task tried to run, but failed N time with errors
    AttemptsWithFailure(Vec<String>),
    /// The task completed successfully
    Complete,
    /// Indicates a task is currently running
    /// Please note that this would be unsafe as we don't update tasks in case the user requests
    /// a shutdown or the program is killed.
    /// Thus we cleanup in-progress tasks by checking if their stored_at time is before the process startup time.
    InProgress(Option<Vec<String>>),
}

impl TaskState {
    pub fn merge_with(&mut self, other: &TaskState) {
        fn merge_vec(mut existing: Vec<String>, new: &Vec<String>) -> Vec<String> {
            existing.extend(new.iter().map(|e| e.clone()));
            existing
        }
        use TaskState::*;
        *self = match (&self, other) {
            (AttemptsWithFailure(existing), AttemptsWithFailure(new)) => {
                AttemptsWithFailure(merge_vec(existing.clone(), new))
            }
            (AttemptsWithFailure(existing), InProgress(None)) => InProgress(Some(existing.clone())),
            (AttemptsWithFailure(_), InProgress(Some(_))) => {
                panic!("One must not create inProgress preloaded with failed attempts, I think :D")
            }
            (InProgress(Some(existing)), AttemptsWithFailure(other)) => {
                AttemptsWithFailure(merge_vec(existing.clone(), other))
            }
            (_, other) => other.clone(),
        };
    }
}

impl Default for TaskState {
    fn default() -> Self {
        TaskState::NotStarted
    }
}

/// Information about a task
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    /// This is set automatically, and can be roughly equivalent to the time a task was finished running (no matter if successfully or failed,
    /// but is generally equivalent to the last time the task was saved
    pub stored_at: SystemTime,
    /// Information about the process that we used to run
    pub process: String,
    /// Information about the process version
    pub version: String,
    pub state: TaskState,
}

impl Default for Task {
    fn default() -> Self {
        Task {
            stored_at: SystemTime::now(),
            process: Default::default(),
            version: Default::default(),
            state: Default::default(),
        }
    }
}

/// An entry in a tar archive, including the most important meta-data
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TarHeader {
    /// The normalized path of the entry. May not be unicode encoded.
    pub path: Vec<u8>,
    /// The size of the file in bytes
    pub size: u64,
    /// The type of entry, to be analyzed with tar::EntryType
    pub entry_type: u8,
}

/// Append-variant-only data structure, otherwise migrations are needed
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TaskResult {
    /// A dummy value just so that we can have a default value
    None,
    /// Most interesting information about an unpacked crate
    ExplodedCrate {
        /// Meta data of all entries in the crate
        entries_meta_data: Vec<TarHeader>,
        /// The actual content of selected files, usually README, License and Cargo.* files
        /// Note that these are also present in entries_meta_data.
        selected_entries: Vec<(TarHeader, Vec<u8>)>,
    },
    /// A download with meta data and the downloaded blob itself
    Download {
        kind: String,
        url: String,
        content_length: u32,
        /// The content type, it's optional because it might not be set (even though it should)
        content_type: Option<String>,
    },
}

impl Default for TaskResult {
    fn default() -> Self {
        TaskResult::None
    }
}

impl<'a> From<&crates_index_diff::CrateVersion> for CrateVersion<'a> {
    fn from(
        crates_index_diff::CrateVersion {
            name,
            kind,
            version,
            checksum,
            features,
            dependencies,
        }: &crates_index_diff::CrateVersion,
    ) -> Self {
        CrateVersion {
            name: name.clone().into(),
            kind: *kind,
            version: version.clone().into(),
            checksum: checksum.clone().into(),
            features: HashMap::from_iter(features.iter().map(|(k, v)| {
                (
                    k.to_owned().into(),
                    v.iter().map(|v| v.to_owned().into()).collect(),
                )
            })),
            dependencies: dependencies.iter().map(Into::into).collect(),
        }
    }
}
