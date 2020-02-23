use crate::{error::Result, model, persistence};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const GENERATOR_VERSION: &str = "1.0.0";

pub struct Generator;

impl Generator {
    pub async fn merge_reports(reports: async_std::sync::Receiver<()>) -> Result<()> {
        while let Some(report) = reports.recv().await {
            drop(report);
        }
        Ok(())
    }

    pub async fn write_files(
        _db: persistence::Db,
        out_dir: PathBuf,
        krates: Vec<(sled::IVec, model::Crate<'_>)>,
        mut progress: prodash::tree::Item,
    ) -> Result<()> {
        use std::os::unix::fs;
        for (k, c) in krates.into_iter() {
            let name = to_name(&k);
            let c: model::Crate = c;
            let mut p = progress.add_child(name);
            p.init(Some(c.versions.len() as u32), Some("versions"));

            for (vid, v) in c.versions.iter().enumerate() {
                p.set((vid + 1) as u32);
                let out_file = output_file_html(out_dir.as_ref(), name, &v);
                tokio::fs::create_dir_all(out_file.parent().expect("parent dir for file")).await?;

                let mut marker = out_file.clone();
                marker.set_file_name(GENERATOR_VERSION);
                if !marker
                    .symlink_metadata()
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    generate_single_file(&out_file).await?;
                    fs::symlink(out_file.file_name().expect("filename"), &marker)?;
                }
            }
        }
        Ok(())
    }
}

async fn generate_single_file(out: &Path) -> Result<()> {
    tokio::fs::OpenOptions::new()
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
