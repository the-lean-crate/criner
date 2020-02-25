use crate::{error::Result, model, persistence};
use std::path::{Path, PathBuf};

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
        _db: persistence::Db,
        out_dir: PathBuf,
        krates: Vec<(sled::IVec, sled::IVec)>,
        mut p: prodash::tree::Item,
    ) -> ReportResult {
        for (k, c) in krates.into_iter() {
            let name = to_name(&k);
            let c: model::Crate = c.into();
            p.init(Some(c.versions.len() as u32), Some("versions"));
            p.set_name(name);

            for (vid, v) in c.versions.iter().enumerate() {
                p.set((vid + 1) as u32);
                let out_file = output_file_html(out_dir.as_ref(), name, &v);
                let mut marker = out_file.clone();
                marker.set_file_name(GENERATOR_VERSION);
                if !async_std::fs::symlink_metadata(&marker)
                    .await
                    .ok()
                    .map(|f| f.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    async_std::fs::create_dir_all(out_file.parent().expect("parent dir for file"))
                        .await?;
                    generate_single_file(&out_file).await?;
                    async_std::os::unix::fs::symlink(
                        out_file.file_name().expect("filename"),
                        &marker,
                    )
                    .await?;
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
