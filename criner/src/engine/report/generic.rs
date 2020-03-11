use crate::{
    error::Result,
    model, persistence,
    persistence::{new_key_insertion, ReportsTree},
};
use async_trait::async_trait;
use rusqlite::{params, TransactionBehavior};
use std::path::{Path, PathBuf};

#[async_trait]
pub trait Aggregate {
    fn merge(self, other: Self) -> Self;
    async fn complete_all(self, out_dir: PathBuf, progress: prodash::tree::Item) -> Result<()>;
    async fn complete_crate(
        &mut self,
        out_dir: &Path,
        crate_name: &str,
        progress: &mut prodash::tree::Item,
    ) -> Result<()>;
}

#[async_trait]
pub trait Generator {
    type Report: Aggregate + Send + Sync;
    type DBResult: Send;

    fn name() -> &'static str;
    fn version() -> &'static str;

    fn fq_result_key(crate_name: &str, crate_version: &str, key_buf: &mut String);
    fn fq_report_key(crate_name: &str, crate_version: &str, key_buf: &mut String) {
        ReportsTree::key_buf(
            crate_name,
            crate_version,
            Self::name(),
            Self::version(),
            key_buf,
        );
    }

    fn get_result(
        connection: persistence::ThreadSafeConnection,
        crate_name: &str,
        crate_version: &str,
        key_buf: &mut String,
    ) -> Result<Option<Self::DBResult>>;

    async fn merge_reports(
        out_dir: PathBuf,
        mut progress: prodash::tree::Item,
        reports: async_std::sync::Receiver<Result<Option<Self::Report>>>,
    ) -> Result<()> {
        progress.init(None, Some("reports"));
        let mut report = None;
        let mut count = 0;
        while let Some(result) = reports.recv().await {
            count += 1;
            progress.set(count);
            match result {
                Ok(Some(new_report)) => report = report.map(|r: Self::Report| r.merge(new_report)),
                Ok(None) => {}
                Err(err) => {
                    progress.fail(format!("report failed: {}", err));
                }
            };
        }
        if let Some(report) = report {
            report.complete_all(out_dir, progress).await?;
        }
        Ok(())
    }

    async fn generate_single_file(
        out: &Path,
        crate_name: &str,
        crate_version: &str,
        result: Self::DBResult,
        _previous_report: Option<&Self::Report>,
        progress: &mut prodash::tree::Item,
    ) -> Result<Self::Report>;

    async fn write_files(
        db: persistence::Db,
        out_dir: PathBuf,
        krates: Vec<(String, Vec<u8>)>,
        mut progress: prodash::tree::Item,
    ) -> Result<Option<Self::Report>> {
        let mut report = None;
        let mut results_to_update = Vec::new();
        {
            let connection = db.open_connection()?;
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
                    Self::fq_report_key(&name, &version, &mut key_buf);

                    if !reports.is_done(&key_buf) {
                        let reports_key = key_buf.clone();
                        key_buf.clear();

                        if let Some(result) =
                            Self::get_result(connection.clone(), &name, &version, &mut key_buf)?
                        {
                            let out_file = output_file_html(&out_dir, &name, &version);
                            async_std::fs::create_dir_all(
                                out_file.parent().expect("parent dir for file"),
                            )
                            .await?;
                            let new_report = Self::generate_single_file(
                                &out_file,
                                &name,
                                &version,
                                result,
                                report.as_ref(),
                                &mut progress,
                            )
                            .await?;
                            report = report.map(|r| r.merge(new_report));

                            results_to_update.push(reports_key);
                        }
                    }
                }
                report = if let Some(mut report) = report {
                    report
                        .complete_crate(&out_dir, &name, &mut progress)
                        .await?;
                    Some(report)
                } else {
                    None
                };
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
        Ok(report)
    }
}

fn output_file_html(base: &Path, name: &str, version: &str) -> PathBuf {
    base.join(name).join(version).join("index.html")
}
