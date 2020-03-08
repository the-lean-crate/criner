use crate::{error::Result, model::TaskResult, persistence::Db};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub struct Generator;

pub enum Report {
    None,
}

#[async_trait]
impl super::generic::Aggregate for Report {
    fn aggregate(self, other: Self) -> Self {
        other
    }

    async fn complete(self, _out_dir: PathBuf, _progress: prodash::tree::Item) -> Result<()> {
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

    fn name() -> &'static str {
        "waste"
    }

    fn version() -> &'static str {
        "1.0.0"
    }

    fn fq_result_key(crate_name: &str, crate_version: &str, key_buf: &mut String) {
        let dummy_task = crate::engine::work::cpubound::default_persisted_extraction_task();
        let dummy_result = crate::model::TaskResult::ExplodedCrate {
            entries_meta_data: Default::default(),
            selected_entries: Default::default(),
        };
        dummy_result.fq_key(crate_name, crate_version, &dummy_task, key_buf);
    }

    async fn generate_single_file(
        _db: &Db,
        out: &Path,
        _result: TaskResult,
        _report: Report,
    ) -> Result<Self::Report> {
        use async_std::prelude::*;
        async_std::fs::OpenOptions::new()
            .truncate(true)
            .write(true)
            .create(true)
            .open(out)
            .await?
            .write_all("hello world".as_bytes())
            .await
            .map_err(crate::Error::from)?;
        Ok(Report::None)
    }
}
