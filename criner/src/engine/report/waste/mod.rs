use crate::persistence::TableAccess;
use crate::{error::Result, model::TaskResult, persistence};
use async_trait::async_trait;
use std::path::Path;

mod report;
pub use report::*;

pub struct Generator;

// NOTE: When multiple reports should be combined, this must become a compound generator which combines
// multiple implementations into one, statically.
#[async_trait]
impl super::generic::Generator for Generator {
    type Report = Report;
    type DBResult = TaskResult;

    fn name() -> &'static str {
        "waste"
    }

    fn version() -> &'static str {
        "1.0.0"
    }

    fn fq_result_key(crate_name: &str, crate_version: &str, key_buf: &mut String) {
        let dummy_task = crate::engine::work::cpubound::default_persisted_extraction_task();
        let dummy_result = TaskResult::ExplodedCrate {
            entries_meta_data: Default::default(),
            selected_entries: Default::default(),
        };
        dummy_result.fq_key(crate_name, crate_version, &dummy_task, key_buf);
    }

    fn get_result(
        connection: persistence::ThreadSafeConnection,
        crate_name: &str,
        crate_version: &str,
        key_buf: &mut String,
    ) -> Result<Option<TaskResult>> {
        Self::fq_result_key(crate_name, crate_version, key_buf);
        let table = persistence::TaskResultTable { inner: connection };
        table.get(&key_buf)
    }

    async fn generate_single_file(
        out: &Path,
        crate_name: &str,
        crate_version: &str,
        result: TaskResult,
        _progress: &mut prodash::tree::Item,
    ) -> Result<Self::Report> {
        use async_std::prelude::*;
        use horrorshow::Template;

        let report = Report::from_result(crate_name, crate_version, result);
        let mut buf = String::new();
        report.clone().write_to_string(&mut buf)?;

        async_std::fs::OpenOptions::new()
            .truncate(true)
            .write(true)
            .create(true)
            .open(out)
            .await?
            .write_all(prettify(buf).as_slice())
            .await
            .map_err(crate::Error::from)?;
        Ok(report)
    }
}

pub fn prettify(xml: String) -> Vec<u8> {
    let mut rd = quick_xml::Reader::from_reader(std::io::Cursor::new(xml.as_bytes()));
    rd.trim_text(true);
    let mut out = quick_xml::Writer::new_with_indent(std::io::Cursor::new(Vec::new()), b' ', 4);
    let mut buf = Vec::new();
    while let Ok(ev) = rd.read_event(&mut buf) {
        match ev {
            quick_xml::events::Event::Eof => break,
            ev => out.write_event(ev).expect("to never run out of memory"),
        };
        buf.clear();
    }
    out.into_inner().into_inner()
}

#[cfg(test)]
mod report_test;
