use crate::model::{CrateVersion, Task, TaskResult};

pub const KEY_SEP_CHAR: char = ':';

pub trait Keyed {
    /// TODO: without sled, we can use strings right away
    fn key_buf(&self, buf: &mut String);
    fn key(&self) -> String {
        let mut buf = String::with_capacity(16);
        self.key_buf(&mut buf);
        buf
    }
}

impl Task {
    pub fn key_from(process: &str, buf: &mut String) {
        buf.push_str(process);
    }
}

impl Keyed for Task {
    fn key_buf(&self, buf: &mut String) {
        Task::key_from(&self.process, buf)
    }
}

impl Keyed for crates_index_diff::CrateVersion {
    fn key_buf(&self, buf: &mut String) {
        CrateVersion::key_from(&self.name, &self.version, buf)
    }
}

impl Keyed for CrateVersion {
    fn key_buf(&self, buf: &mut String) {
        CrateVersion::key_from(&self.name, &self.version, buf)
    }
}

impl Keyed for TaskResult {
    fn key_buf(&self, buf: &mut String) {
        match self {
            TaskResult::Download { kind, .. } => buf.push_str(kind),
            TaskResult::None | TaskResult::ExplodedCrate { .. } => {}
        }
    }
}

impl CrateVersion {
    pub fn key_from(name: &str, version: &str, buf: &mut String) {
        buf.push_str(name);
        buf.push(KEY_SEP_CHAR);
        buf.push_str(version);
    }
}
