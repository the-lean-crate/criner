use crate::persistence::{new_key_insertion, ReportsTree, TreeAccess};
use crate::{error::Result, model, persistence};
use rusqlite::{params, TransactionBehavior};
use std::path::{Path, PathBuf};

const GENERATOR_NAME: &str = "waste";
const GENERATOR_VERSION: &str = "1.0.0";

pub struct Generator;

pub type ReportResult = Result<()>;

impl Generator {
    pub async fn merge_reports(
        mut progress: prodash::tree::Item,
        reports: async_std::sync::Receiver<ReportResult>,
    ) -> Result<()> {
        progress.init(None, Some("reports"));
        let mut count = 0;
        while let Some(report) = reports.recv().await {
            count += 1;
            progress.set(count);
            if let Err(err) = report {
                progress.fail(format!("report failed: {}", err));
            }
        }
        Ok(())
    }

    pub async fn write_files(
        db: persistence::Db,
        out_dir: PathBuf,
        krates: Vec<(String, Vec<u8>)>,
        mut progress: prodash::tree::Item,
    ) -> ReportResult {
        let exrtaction_task_dummy =
            crate::engine::work::cpubound::default_persisted_extraction_task();
        let dummy_extraction_result = crate::model::TaskResult::ExplodedCrate {
            entries_meta_data: Default::default(),
            selected_entries: Default::default(),
        };
        let mut results_to_update = Vec::new();
        {
            let results = db.open_results()?;
            let reports = db.open_reports()?;
            let mut key_buf = String::with_capacity(32);
            // delaying writes works because we don't have overlap on work
            for (name, krate) in krates.into_iter() {
                let c: model::Crate = krate.as_slice().into();
                progress.init(Some(c.versions.len() as u32), Some("versions"));
                progress.set_name(&name);

                for (vid, version) in c.versions.iter().enumerate() {
                    progress.set((vid + 1) as u32);

                    key_buf.clear();
                    ReportsTree::key_buf(
                        &name,
                        &version,
                        GENERATOR_NAME,
                        GENERATOR_VERSION,
                        &mut key_buf,
                    );

                    if !reports.is_done(&key_buf) {
                        // unreachable!("everything is processed, got {}:{}", name, version);
                        let reports_key = key_buf.clone();
                        key_buf.clear();
                        dummy_extraction_result.fq_key(
                            &name,
                            &version,
                            &exrtaction_task_dummy,
                            &mut key_buf,
                        );
                        if let Some(model::TaskResult::ExplodedCrate {
                            entries_meta_data,
                            selected_entries,
                        }) = results.get(&key_buf)?
                        {
                            let out_file = output_file_html(out_dir.as_ref(), &name, &version);
                            async_std::fs::create_dir_all(
                                out_file.parent().expect("parent dir for file"),
                            )
                            .await?;
                            generate_single_file(&out_file, entries_meta_data, selected_entries)
                                .await?;

                            results_to_update.push(reports_key);
                        }
                    }
                }
            }
        }
        if !results_to_update.is_empty() {
            let mut connection = db.open_connection_no_async()?;
            progress.blocked("wait for write lock", None);
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            progress.init(
                Some(results_to_update.len() as u32),
                Some("report done markers written"),
            );
            {
                let mut statement = new_key_insertion(ReportsTree::table_name(), &transaction)?;
                for (kid, key) in results_to_update.iter().enumerate() {
                    statement.execute(params![key])?;
                    progress.set((kid + 1) as u32);
                }
            }
            transaction.commit()?;
        }
        Ok(())
    }
}

async fn generate_single_file<'a>(
    out: &Path,
    _entries_meta_data: Vec<model::TarHeader>,
    _selected_entries: Vec<(model::TarHeader, Vec<u8>)>,
) -> Result<()> {
    use async_std::prelude::*;
    async_std::fs::OpenOptions::new()
        .truncate(true)
        .write(true)
        .create(true)
        .open(out)
        .await?
        .write_all("hello world".as_bytes())
        .await
        .map_err(Into::into)
}

fn output_file_html(base: &Path, name: &str, version: &str) -> PathBuf {
    base.join(name).join(version).join("index.html")
}
