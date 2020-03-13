use crate::{
    error::Result,
    model, persistence,
    persistence::{new_key_insertion, ReportsTree},
};
use async_trait::async_trait;
use rusqlite::{params, TransactionBehavior};
use std::path::{Path, PathBuf};

#[async_trait]
pub trait Aggregate
where
    Self: Sized,
{
    fn merge(self, other: Self) -> Self;
    async fn complete(&mut self, out_dir: &Path, progress: &mut prodash::tree::Item) -> Result<()>;
    async fn load_previous_state(
        &self,
        out_dir: &Path,
        progress: &mut prodash::tree::Item,
    ) -> Option<Self>;
    async fn store_current_state(
        &self,
        out_dir: &Path,
        progress: &mut prodash::tree::Item,
    ) -> Result<()>;
}

#[async_trait]
pub trait Generator {
    type Report: Aggregate + Send + Sync + Clone;
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
        cache_dir: Option<PathBuf>,
        mut progress: prodash::tree::Item,
        reports: async_std::sync::Receiver<Result<Option<Self::Report>>>,
    ) -> Result<()> {
        let mut report = None::<Self::Report>;
        let mut count = 0;
        while let Some(result) = reports.recv().await {
            count += 1;
            progress.set(count);
            match result {
                Ok(Some(new_report)) => {
                    report = Some(match report {
                        Some(report) => report.merge(new_report),
                        None => new_report,
                    })
                }
                Ok(None) => {}
                Err(err) => {
                    progress.fail(format!("report failed: {}", err));
                }
            };
        }
        if let Some(mut report) = report {
            let previous_report = match cache_dir.as_ref() {
                Some(cd) => report.load_previous_state(&cd, &mut progress).await,
                None => None,
            };
            report = match previous_report {
                Some(previous_report) => previous_report.merge(report),
                None => report,
            };
            report.complete(&out_dir, &mut progress).await?;
            if let Some(cd) = cache_dir {
                report.store_current_state(&cd, &mut progress).await?;
            }
        }
        Ok(())
    }

    async fn generate_single_file(
        out: &Path,
        crate_name: &str,
        crate_version: &str,
        result: Self::DBResult,
        progress: &mut prodash::tree::Item,
    ) -> Result<Self::Report>;

    async fn write_files(
        db: persistence::Db,
        out_dir: PathBuf,
        cache_dir: Option<PathBuf>,
        krates: Vec<(String, Vec<u8>)>,
        mut progress: prodash::tree::Item,
    ) -> Result<Option<Self::Report>> {
        let mut chunk_report = None::<Self::Report>;
        let mut results_to_update = Vec::new();
        {
            let connection = db.open_connection()?;
            let reports = db.open_reports()?;
            let mut key_buf = String::with_capacity(32);
            // delaying writes works because we don't have overlap on work
            for (name, krate) in krates.into_iter() {
                let c: model::Crate = krate.as_slice().into();
                let crate_dir = crate_dir(&out_dir, &name);
                async_std::fs::create_dir_all(&crate_dir).await?;
                progress.init(Some(c.versions.len() as u32), Some("versions"));
                progress.set_name(&name);

                let mut crate_report = None::<Self::Report>;
                for (vid, version) in c.versions.iter().enumerate() {
                    progress.set((vid + 1) as u32);

                    key_buf.clear();
                    Self::fq_report_key(&name, &version, &mut key_buf);

                    // If we have no cache, assume we are globbed (yes, I know…sigh), so always produce reports
                    // but don't invalidate data in caches by reading or writing them. Mostly used for testing
                    // as it creates a sub-report, every time without having to fiddle with the
                    // reports_done marker table.
                    if cache_dir.is_none() || !reports.is_done(&key_buf) {
                        let reports_key = key_buf.clone();
                        key_buf.clear();

                        if let Some(result) =
                            Self::get_result(connection.clone(), &name, &version, &mut key_buf)?
                        {
                            let out_file = output_file_html(&crate_dir, &version);
                            let version_report = Self::generate_single_file(
                                &out_file,
                                &name,
                                &version,
                                result,
                                &mut progress,
                            )
                            .await?;
                            crate_report = Some(match crate_report {
                                Some(crate_report) => crate_report.merge(version_report),
                                None => version_report,
                            });

                            results_to_update.push(reports_key);
                        }
                    }
                }
                if let Some(mut crate_report) = crate_report {
                    let previous_state = match cache_dir.as_ref() {
                        Some(cd) => crate_report.load_previous_state(&cd, &mut progress).await,
                        None => None,
                    };
                    match previous_state {
                        Some(previous_state) => {
                            let mut absolute_state = previous_state.merge(crate_report.clone());
                            absolute_state.complete(&crate_dir, &mut progress).await?;
                            if let Some(cd) = cache_dir.as_ref() {
                                absolute_state
                                    .store_current_state(&cd, &mut progress)
                                    .await?;
                            };
                        }
                        None => {
                            crate_report.complete(&crate_dir, &mut progress).await?;
                            if let Some(cd) = cache_dir.as_ref() {
                                crate_report.store_current_state(&cd, &mut progress).await?;
                            }
                        }
                    }
                    chunk_report = Some(match chunk_report {
                        Some(chunk_report) => chunk_report.merge(crate_report),
                        None => crate_report,
                    });
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
        Ok(chunk_report)
    }
}

fn crate_dir(base: &Path, crate_name: &str) -> PathBuf {
    base.join(crate_name)
}

fn output_file_html(crate_dir: &Path, version: &str) -> PathBuf {
    crate_dir.join(format!("{}.html", version))
}
