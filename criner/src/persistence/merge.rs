use crate::model;
use crate::model::Task;

pub trait Merge<T> {
    fn merge(self, other: T) -> Self;
}

impl Merge<model::Task> for model::Task {
    fn merge(self, mut other: Task) -> Self {
        other.state = self.state.merge(other.state);
        other
    }
}

impl Merge<model::TaskState> for model::TaskState {
    fn merge(mut self, other: model::TaskState) -> Self {
        fn merge_vec(mut existing: Vec<String>, new: Vec<String>) -> Vec<String> {
            existing.extend(new.into_iter());
            existing
        }
        use model::TaskState::*;
        self = match (self, other) {
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
