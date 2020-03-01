use crate::model::{CrateVersion, Task, TaskResult};
use crate::Result;

pub const KEY_SEP: u8 = b':';
pub const KEY_SEP_CHAR: char = ':';

pub trait Keyed {
    fn key_bytes_buf(&self, buf: &mut Vec<u8>);
    fn key_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(16);
        self.key_bytes_buf(&mut buf);
        buf
    }
    fn key_string(&self) -> Result<String> {
        String::from_utf8(self.key_bytes()).map_err(Into::into)
    }
}

impl<'a> Task<'a> {
    pub fn key_from(process: &str, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&process.as_bytes());
    }
}

impl<'a> Keyed for Task<'a> {
    fn key_bytes_buf(&self, buf: &mut Vec<u8>) {
        Task::key_from(&self.process, buf)
    }
}

impl Keyed for crates_index_diff::CrateVersion {
    fn key_bytes_buf(&self, buf: &mut Vec<u8>) {
        CrateVersion::key_from(&self.name, &self.version, buf)
    }
}

impl<'a> Keyed for CrateVersion<'a> {
    fn key_bytes_buf(&self, buf: &mut Vec<u8>) {
        CrateVersion::key_from(&self.name, &self.version, buf)
    }
}

impl<'a> Keyed for TaskResult<'a> {
    fn key_bytes_buf(&self, buf: &mut Vec<u8>) {
        match self {
            TaskResult::Download { kind, .. } => buf.extend_from_slice(kind.as_bytes()),
            TaskResult::None | TaskResult::ExplodedCrate { .. } => {}
        }
    }
}
