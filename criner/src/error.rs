use crates_index_diff::git2;
use humantime;
use rmp_serde;
use std::{fmt, time};

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
        Message(d: String) {
            display("{}", d)
        }
        InvalidHeader(d: &'static str) {
            display("{}", d)
        }
        HttpStatus(status: http::StatusCode) {
            display("{}", status)
        }
        DeadlineExceeded(d: FormatDeadline) {
            display("Stopped computation as deadline was reached {}.", d)
        }
        Interrupted {
            display("Interrupt or termination signal received")
        }
        Timeout(d: std::time::Duration, msg: String) {
            display("{} - timeout after {:?}.", msg, d)
        }
        RmpSerdeEncode(err: rmp_serde::encode::Error) {
            from()
            source(err)
        }
        Git2(err: git2::Error) {
            from()
            source(err)
        }
        Io(err: std::io::Error) {
            from()
            source(err)
        }
        FromUtf8(err: std::string::FromUtf8Error) {
            from()
            source(err)
        }
        Reqwest(err: reqwest::Error) {
            from()
            source(err)
        }
        ParseInt(err: std::num::ParseIntError) {
            from()
            source(err)
        }
        Rusqlite(err: rusqlite::Error) {
            from()
            source(err)
        }
        GlobSet(err: globset::Error) {
            from()
            source(err)
        }
        Horrorshow(err: horrorshow::Error) {
            from()
            source(err)
        }
        SystemTime(err: std::time::SystemTimeError) {
            from()
            source(err)
        }
        StripPrefixError(err: std::path::StripPrefixError) {
            from()
            source(err)
        }
        Csv(err: csv::Error) {
            from()
            source(err)
        }
        GlobPattern(err: glob::PatternError) {
            from()
            source(err)
        }
        Glob(err: glob::GlobError) {
            from()
            source(err)
        }
        ChannelSendMessage(msg: &'static str) {
            display("{}: Sending into a closed channel", msg)
        }
    }
}

impl Error {
    pub fn send_msg<T>(msg: &'static str) -> impl FnOnce(async_channel::SendError<T>) -> Error {
        move |_err| Error::ChannelSendMessage(msg)
    }
}
