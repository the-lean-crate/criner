use crate::persistence::TreeAccess;
use crate::{error::Result, model, persistence};
use std::borrow::Cow;
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
        krates: Vec<(sled::IVec, sled::IVec)>,
        mut p: prodash::tree::Item,
    ) -> ReportResult {
        let exrtaction_task_dummy =
            crate::engine::work::cpubound::default_persisted_extraction_task();
        let mut dummy_extraction_result = crate::model::TaskResult::ExplodedCrate {
            entries_meta_data: Default::default(),
            selected_entries: Default::default(),
        };
        let results = db.results();
        let mut key_buf = Vec::with_capacity(32);
        for (krate_key, krate) in krates.into_iter() {
            let name = to_name(&krate_key);
            let c: model::Crate = krate.into();
            p.init(Some(c.versions.len() as u32), Some("versions"));
            p.set_name(name);

            for (vid, version) in c.versions.iter().enumerate() {
                {
                    let key = (
                        name.as_ref(),
                        version.as_ref(),
                        &exrtaction_task_dummy,
                        dummy_extraction_result,
                    );
                    key_buf.clear();
                    // persistence::TaskResultTree::key_to_buf(&key, &mut key_buf);
                    key_to_buf(&key, &mut key_buf);
                    dummy_extraction_result = key.3;
                }
                p.set((vid + 1) as u32);
                let out_file = output_file_html(out_dir.as_ref(), name, &version);
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
    _entries_meta_data: Cow<'a, [model::TarHeader<'a>]>,
    _selected_entries: Cow<'a, [(model::TarHeader<'a>, Cow<'a, [u8]>)]>,
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

fn to_name(key: &sled::IVec) -> &str {
    std::str::from_utf8(key).expect("unicode keys")
}

// FIXME: this is a copy of persistence::TaskResultTree, which doens't work as it wants &'static str, but doesn't tell
// Seems to be some sort of borrow checker bug.
fn key_to_buf<'a>(v: &(&str, &str, &model::Task, model::TaskResult<'a>), buf: &mut Vec<u8>) {
    use persistence::Keyed;
    persistence::TasksTree::key_to_buf(&(v.0, v.1, v.2.clone()), buf);
    buf.push(persistence::KEY_SEP);
    buf.extend_from_slice(v.2.version.as_bytes());
    buf.push(persistence::KEY_SEP);
    v.3.key_bytes_buf(buf);
}
