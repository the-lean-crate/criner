use crate::persistence::TableAccess;
use crate::{error::Result, model::TaskResult, persistence};
use async_trait::async_trait;
use std::path::Path;

pub struct Generator;

mod report {
    use crate::{model::TaskResult, Result};
    use async_trait::async_trait;
    use std::path::{Path, PathBuf};

    #[derive(PartialEq, Eq, Debug)]
    pub enum Report {
        None,
        Version {
            total_size_in_bytes: u64,
            total_files: u64,
            wasted_bytes: u64,
            wasted_files: u64,
            suggested_include: Option<Vec<String>>,
        },
    }

    impl From<TaskResult> for Report {
        fn from(result: TaskResult) -> Report {
            match result {
                TaskResult::ExplodedCrate {
                    entries_meta_data,
                    selected_entries: _,
                } => Report::Version {
                    total_size_in_bytes: entries_meta_data.iter().map(|e| e.size).sum(),
                    total_files: entries_meta_data.len() as u64,
                    wasted_bytes: 0,
                    wasted_files: 0,
                    suggested_include: None,
                },
                _ => unreachable!("need caller to assure we get exploded crates only"),
            }
        }
    }

    #[async_trait]
    impl crate::engine::report::generic::Aggregate for Report {
        fn merge(self, other: Self) -> Self {
            other
        }

        async fn complete_all(
            self,
            _out_dir: PathBuf,
            _progress: prodash::tree::Item,
        ) -> Result<()> {
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

    #[cfg(test)]
    mod from_extract_crate {
        use super::Report;
        use crate::model::TaskResult;

        const GNIR: &[u8] =
            include_bytes!("../../../tests/fixtures/gnir-0.14.0-alpha3-extract_crate-1.0.0.bin");
        const SOVRIN: &[u8] = include_bytes!(
            "../../../tests/fixtures/sovrin-client.0.1.0-179-extract_crate-1.0.0.bin"
        );
        const MOZJS: &[u8] =
            include_bytes!("../../../tests/fixtures/mozjs_sys-0.67.1-extract_crate-1.0.0.bin");

        #[test]
        fn gnir() {
            assert_eq!(
                Report::from(TaskResult::from(GNIR)),
                Report::Version {
                    total_size_in_bytes: 15216510,
                    total_files: 382,
                    wasted_bytes: 0,
                    wasted_files: 0,
                    suggested_include: None
                },
                "correct size and assume people are aware if includes or excludes are present"
            );
        }
        #[test]
        fn sovrin_client() {
            assert_eq!(
                Report::from(TaskResult::from(SOVRIN)),
                Report::Version {
                    total_size_in_bytes: 20283032,
                    total_files: 479,
                    wasted_files: 0,
                    wasted_bytes: 0,
                    suggested_include: None
                },
                "build.rs is used but there are a bunch of extra directories that can be ignored and are not needed by the build, no manual includes/excludes"
            );
        }
        #[test]
        fn mozjs() {
            // todo: filter tests, benches, examples, image file formats, docs, allow everything in src/ , but be aware of tests/specs
            assert_eq!(
                Report::from(TaskResult::from(MOZJS)),
                Report::Version {
                    total_size_in_bytes: 161225785,
                    total_files: 13187,
                    wasted_files: 0,
                    wasted_bytes: 0,
                    suggested_include: None
                },
                "build.rs + excludes in Cargo.toml - this leaves a chance for accidental includes for which we provide an updated exclude list"
            );
        }
    }
}

pub use report::Report;

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
