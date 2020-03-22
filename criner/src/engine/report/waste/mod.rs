use crate::persistence::TableAccess;
use crate::{error::Result, model::TaskResult, persistence};
use async_trait::async_trait;

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

    async fn generate_report(
        crate_name: &str,
        crate_version: &str,
        result: TaskResult,
        _progress: &mut prodash::tree::Item,
    ) -> Result<Self::Report> {
        Ok(Report::from_result(crate_name, crate_version, result))
    }
}

#[cfg(test)]
mod report_test;
