use crate::{error::Result, model, persistence};
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
        krates: Vec<(sled::IVec, sled::IVec)>,
        mut progress: prodash::tree::Item,
    ) -> ReportResult {
        let reports = db.reports();
        for (k, c) in krates.into_iter() {
            let name = to_name(&k);
            let c: model::Crate = c.into();
            let mut p = progress.add_child(name);
            p.init(Some(c.versions.len() as u32), Some("versions"));

            for (vid, version) in c.versions.iter().enumerate() {
                p.set((vid + 1) as u32);
                let key = persistence::ReportsTree::key(
                    name,
                    &version,
                    GENERATOR_NAME,
                    GENERATOR_VERSION,
                );
                if !reports.is_done(&key) {
                    let out_file = output_file_html(out_dir.as_ref(), name, &version);
                    async_std::fs::create_dir_all(out_file.parent().expect("parent dir for file"))
                        .await?;
                    generate_single_file(&out_file).await?;
                    reports.set_done(key);
                }
            }
        }
        Ok(())
    }
}

async fn generate_single_file(out: &Path) -> Result<()> {
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

fn to_name(key: &sled::IVec) -> &str {
    std::str::from_utf8(key).expect("unicode keys")
}
