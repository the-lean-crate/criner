use crate::persistence::TableAccess;
use crate::{error::Result, model::TaskResult, persistence};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub struct Generator;

pub enum Report {
    None,
}

impl From<TaskResult> for Report {
    fn from(_result: TaskResult) -> Report {
        Report::None
    }
}

#[async_trait]
impl super::generic::Aggregate for Report {
    fn merge(self, other: Self) -> Self {
        other
    }

    async fn complete_all(self, _out_dir: PathBuf, _progress: prodash::tree::Item) -> Result<()> {
        Ok(())
    }
    async fn complete_crate(
        &mut self,
        _out_dir: &Path,
        _crate_name: &str,
        _progress: &mut prodash::tree::Item,
    ) -> Result<()> {
        Ok(())
    }
}

impl Default for Report {
    fn default() -> Self {
        Report::None
    }
}

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
        _crate_name: &str,
        _crate_version: &str,
        result: TaskResult,
        _report: &Report,
        _progress: &mut prodash::tree::Item,
    ) -> Result<Self::Report> {
        use async_std::prelude::*;
        let report = Report::from(result);

        async_std::fs::OpenOptions::new()
            .truncate(true)
            .write(true)
            .create(true)
            .open(out)
            .await?
            .write_all("hello world".as_bytes())
            .await
            .map_err(crate::Error::from)?;
        Ok(report)
    }
}
