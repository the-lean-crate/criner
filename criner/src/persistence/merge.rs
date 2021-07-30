use crate::model::{self, Context, CrateVersion, Task};
use crate::utils::parse_semver;

pub trait Merge<T> {
    fn merge(self, other: &T) -> Self;
}

impl Merge<model::Task> for model::Task {
    fn merge(mut self, other: &Task) -> Self {
        let my_state = self.state;
        self = other.clone();
        self.state = my_state.merge(&other.state);
        self
    }
}

impl Merge<model::TaskState> for model::TaskState {
    fn merge(mut self, other: &model::TaskState) -> Self {
        fn merge_vec(mut existing: Vec<String>, new: &[String]) -> Vec<String> {
            existing.extend(new.iter().cloned());
            existing
        }
        use model::TaskState::*;
        self = match (&self, other) {
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
        self
    }
}

impl Merge<model::Context> for model::Context {
    fn merge(self, other: &Context) -> Self {
        self + other
    }
}

fn sort_semver(versions: &mut Vec<String>) {
    versions.sort_by_key(|v| parse_semver(v));
}

impl Merge<model::CrateVersion> for model::Crate {
    fn merge(mut self, other: &CrateVersion) -> Self {
        if !self.versions.contains(&other.version) {
            self.versions.push(other.version.to_owned());
        }
        sort_semver(&mut self.versions);
        self
    }
}

impl model::Crate {
    pub fn merge_mut(&mut self, other: &CrateVersion) -> &mut model::Crate {
        if !self.versions.contains(&other.version) {
            self.versions.push(other.version.to_owned());
        }
        sort_semver(&mut self.versions);
        self
    }
}
