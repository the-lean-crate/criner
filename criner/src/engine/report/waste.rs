use crate::persistence::TreeAccess;
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
        db: persistence::Db,
        out_dir: PathBuf,
        krates: Vec<(String, Vec<u8>)>,
        mut p: prodash::tree::Item,
    ) -> ReportResult {
        let exrtaction_task_dummy =
            crate::engine::work::cpubound::default_persisted_extraction_task();
        let dummy_extraction_result = crate::model::TaskResult::ExplodedCrate {
            entries_meta_data: Default::default(),
            selected_entries: Default::default(),
        };
        let results = db.open_results()?;
        let mut key_buf = String::with_capacity(32);
        for (name, krate) in krates.into_iter() {
            let c: model::Crate = krate.as_slice().into();
            p.init(Some(c.versions.len() as u32), Some("versions"));
            p.set_name(&name);

            for (vid, version) in c.versions.iter().enumerate() {
                key_buf.clear();
                dummy_extraction_result.fq_key(
                    &name,
                    &version,
                    &exrtaction_task_dummy,
                    &mut key_buf,
                );
                p.set((vid + 1) as u32);
                let out_file = output_file_html(out_dir.as_ref(), &name, &version);
                let mut marker = out_file.clone();
                marker.set_file_name(GENERATOR_VERSION);
                if !async_std::fs::symlink_metadata(&marker)
                    .await
                    .ok()
                    .map(|f| f.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    if let Some(model::TaskResult::ExplodedCrate {
                        entries_meta_data,
                        selected_entries,
                    }) = results.get(&key_buf)?
                    {
                        async_std::fs::create_dir_all(
                            out_file.parent().expect("parent dir for file"),
                        )
                        .await?;
                        generate_single_file(&out_file, entries_meta_data, selected_entries)
                            .await?;
                        async_std::os::unix::fs::symlink(
                            out_file.file_name().expect("filename"),
                            &marker,
                        )
                        .await?;
                    }
                }
            }
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
