use crates_index_diff::git2;
use humantime;
use rmp_serde;
use sled;
use std::fmt;
use std::time;

#[derive(Debug)]
pub struct FormatDeadline(pub time::SystemTime);

impl fmt::Display for FormatDeadline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::result::Result<(), fmt::Error> {
        let now = time::SystemTime::now();
        write!(
            f,
            "{} ago at {}",
            humantime::format_duration(now.duration_since(self.0).unwrap_or_default()),
            humantime::format_rfc3339(now)
        )
    }
}

pub type Result<T> = std::result::Result<T, Error>;

quick_error! {
    #[derive(Debug)]
    pub enum Error {
        Bug(d: &'static str) {
            display("{}", d)
        }
        InvalidHeader(d: &'static str) {
            display("{}", d)
        }
        DeadlineExceeded(d: FormatDeadline) {
            display("Stopped computation as deadline was reached {}.", d)
        }
        Spawn(err: futures::task::SpawnError) {
            from()
            cause(err)
        }
        RmpSerdeEncode(err: rmp_serde::encode::Error) {
            from()
            cause(err)
        }
        Git2(err: git2::Error) {
            from()
            cause(err)
        }
        Sled(err: sled::Error) {
            from()
            cause(err)
        }
        Io(err: std::io::Error) {
            from()
            cause(err)
        }
        FromUtf8(err: std::string::FromUtf8Error) {
            from()
            cause(err)
        }
        Reqwest(err: reqwest::Error) {
            from()
            cause(err)
        }
        ParseInt(err: std::num::ParseIntError) {
            from()
            cause(err)
        }
    }
}
