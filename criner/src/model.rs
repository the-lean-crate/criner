pub use crate::engine::report::waste::TarHeader;
use serde_derive::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::{collections::HashMap, ops::Add, time::Duration, time::SystemTime};

/// Represents a top-level crate and associated information
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Crate {
    /// All versions published to crates.io, guaranteed to be sorted so that the most recent version is last.
    /// The format is as specified in Cargo.toml:version
    pub versions: Vec<String>,
}

impl From<CrateVersion> for Crate {
    fn from(v: CrateVersion) -> Self {
        Crate {
            versions: vec![v.version],
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
                fetch_crate_versions: self.durations.fetch_crate_versions + rhs.durations.fetch_crate_versions,
            },
        }
    }
}

/// A single dependency of a specific crate version
#[derive(Clone, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Debug)]
pub struct Dependency {
    /// The crate name
    pub name: String,
    /// The version the parent crate requires of this dependency
    #[serde(rename = "req")]
    pub required_version: String,
    /// All cargo features configured by the parent crate
    pub features: Vec<String>,
    /// True if this is an optional dependency
    pub optional: bool,
    /// True if default features are enabled
    pub default_features: bool,
    /// The name of the build target
    pub target: Option<String>,
    /// The kind of dependency, usually 'normal' or 'dev'
    pub kind: Option<String>,
    /// The package this crate is contained in
    pub package: Option<String>,
}

impl From<crates_index_diff::Dependency> for Dependency {
    fn from(v: crates_index_diff::Dependency) -> Self {
        let crates_index_diff::Dependency {
            name,
            required_version,
            features,
            optional,
            default_features,
            target,
            kind,
            package,
        } = v;
        Dependency {
            name: name.to_string(),
            required_version: required_version.to_string(),
            features,
            optional,
            default_features,
            target: target.map(|t| t.to_string()),
            kind: kind.map(|k| {
                match k {
                    crates_index_diff::DependencyKind::Normal => "normal",
                    crates_index_diff::DependencyKind::Dev => "dev",
                    crates_index_diff::DependencyKind::Build => "build",
                }
                .to_owned()
            }),
            package: package.map(|p| p.to_string()),
        }
    }
}

/// Identify a kind of change that occurred to a crate
#[derive(Clone, Copy, Ord, PartialOrd, Eq, PartialEq, Debug)]
pub enum ChangeKind {
    /// A crate version was added
    Added,
    /// A crate version was added or it was unyanked.
    Yanked,
}

impl Default for ChangeKind {
    fn default() -> Self {
        ChangeKind::Added
    }
}

impl<'de> serde::Deserialize<'de> for ChangeKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> ::serde::de::Visitor<'de> for Visitor {
            type Value = ChangeKind;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("boolean")
            }
            fn visit_bool<E>(self, value: bool) -> Result<ChangeKind, E>
            where
                E: ::serde::de::Error,
            {
                if value {
                    Ok(ChangeKind::Yanked)
                } else {
                    Ok(ChangeKind::Added)
                }
            }
        }
        deserializer.deserialize_bool(Visitor)
    }
}

impl serde::Serialize for ChangeKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bool(self == &ChangeKind::Yanked)
    }
}

/// Pack all information we know about a change made to a version of a crate.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct CrateVersion {
    /// The crate name, i.e. `clap`.
    pub name: String,
    /// The kind of change.
    #[serde(rename = "yanked")]
    pub kind: ChangeKind,
    /// The semantic version of the crate.
    #[serde(rename = "vers")]
    pub version: String,
    /// The checksum over the crate archive
    #[serde(rename = "cksum")]
    pub checksum: String,
    /// All cargo features
    pub features: HashMap<String, Vec<String>>,
    /// All crate dependencies
    #[serde(rename = "deps")]
    pub dependencies: Vec<Dependency>,
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
    pub fn is_complete(&self) -> bool {
        matches!(self, TaskState::Complete)
    }
    pub fn merge_with(&mut self, other: &TaskState) {
        fn merge_vec(mut existing: Vec<String>, new: &[String]) -> Vec<String> {
            existing.extend(new.iter().cloned());
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

impl Task {
    // NOTE: Racy if task should be spawned based on the outcome, only for tasks with no contention!
    pub fn can_be_started(&self, startup_time: std::time::SystemTime) -> bool {
        match self.state {
            TaskState::NotStarted | TaskState::AttemptsWithFailure(_) => true,
            TaskState::InProgress(_) => startup_time > self.stored_at,
            _ => false,
        }
    }
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
        /// The actual content of selected files, Cargo.*, build.rs and lib/main
        /// IMPORTANT: This file may be partial and limited in size unless it is Cargo.toml, which
        /// is always complete.
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

impl TryFrom<crates_index_diff::Change> for CrateVersion {
    type Error = ();

    fn try_from(v: crates_index_diff::Change) -> Result<Self, Self::Error> {
        let v = match v {
            crates_index_diff::Change::CrateDeleted { .. } | crates_index_diff::Change::VersionDeleted(_) => {
                // ignore for now
                return Err(());
            }
            crates_index_diff::Change::Unyanked(v)
            | crates_index_diff::Change::Added(v)
            | crates_index_diff::Change::AddedAndYanked(v)
            | crates_index_diff::Change::Yanked(v) => v,
        };
        let crates_index_diff::CrateVersion {
            name,
            yanked,
            version,
            checksum,
            features,
            dependencies,
        } = v;
        Ok(CrateVersion {
            name: name.to_string(),
            kind: yanked.then(|| ChangeKind::Yanked).unwrap_or(ChangeKind::Added),
            version: version.to_string(),
            checksum: hex::encode(checksum),
            features,
            dependencies: dependencies.into_iter().map(Into::into).collect(),
        })
    }
}

pub mod db_dump {
    use serde_derive::{Deserialize, Serialize};
    use std::time::SystemTime;

    pub type Id = u32;
    pub type GitHubId = i32;

    /// Identifies a kind of actor
    #[derive(Clone, Copy, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Debug)]
    pub enum ActorKind {
        User,
        Team,
    }

    #[derive(Clone, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Debug)]
    pub struct Actor {
        /// The id used by crates.io
        pub crates_io_id: Id,
        /// Whether actor is a user or a team
        pub kind: ActorKind,
        /// The URL to the GitHub avatar
        pub github_avatar_url: String,
        /// The ID identifying a user on GitHub
        pub github_id: GitHubId,
        /// The GitHUb login name
        pub github_login: String,
        /// The users given name
        pub name: Option<String>,
    }

    #[derive(Clone, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Debug)]
    pub struct Feature {
        /// The name of the feature
        pub name: String,
        /// The crates the feature depends on
        pub crates: Vec<String>,
    }

    #[derive(Clone, Default, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Debug)]
    pub struct Person {
        pub name: String,
        pub email: Option<String>,
    }

    /// A crate version from the crates-io db dump, containing additional meta data
    #[derive(Clone, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Debug)]
    pub struct CrateVersion {
        /// The size of the crate in bytes, compressed
        pub crate_size: Option<u32>,
        /// The time when the first crate version was published
        pub created_at: SystemTime,
        /// The time at which the most recent create version was published
        pub updated_at: SystemTime,
        /// The amount of downloads of all create version in all time
        pub downloads: u32,
        /// Features specified in Cargo.toml
        pub features: Vec<Feature>,
        /// The license type
        pub license: String,
        /// The semantic version associated with this version
        pub semver: String,
        /// The actor that published the version
        pub published_by: Option<Actor>,
        /// If true, the version was yanked
        pub is_yanked: bool,
    }

    #[derive(Clone, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Debug)]
    pub struct Keyword {
        pub name: String,
        /// The amount of crates using this keyword
        pub crates_count: u32,
    }

    #[derive(Clone, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Debug)]
    pub struct Category {
        pub name: String,
        /// The amount of crates using this keyword
        pub crates_count: u32,
        pub description: String,
        pub path: String,
        pub slug: String,
    }

    /// Everything crates.io knows about a crate in one neat package
    #[derive(Clone, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Debug)]
    pub struct Crate {
        pub name: String,
        /// The time at which this record was updated, i.e. how recent it is.
        pub stored_at: SystemTime,
        pub created_at: SystemTime,
        pub updated_at: SystemTime,
        pub description: Option<String>,
        pub documentation: Option<String>,
        pub downloads: u64,
        pub homepage: Option<String>,
        pub readme: Option<String>,
        pub repository: Option<String>,
        /// Versions, sorted by semantic version
        pub versions: Vec<CrateVersion>,
        pub keywords: Vec<Keyword>,
        pub categories: Vec<Category>,
        pub created_by: Option<Actor>,
        pub owners: Vec<Actor>,
    }
}
