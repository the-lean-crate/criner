use crate::{error::Result, model, persistence};
use std::{fs::File, io::BufReader, path::PathBuf, time::SystemTime};

pub struct ExtractRequest {
    pub download_task: model::TaskOwned,
    pub crate_name: String,
    pub crate_version: String,
}

pub fn default_persisted_download_task() -> model::Task<'static> {
    const TASK_NAME: &str = "extract_crate";
    const TASK_VERSION: &str = "1.0.0";
    model::Task {
        stored_at: SystemTime::now(),
        process: TASK_NAME.into(),
        version: TASK_VERSION.into(),
        state: Default::default(),
    }
}

pub async fn processor(
    db: persistence::Db,
    mut progress: prodash::tree::Item,
    r: async_std::sync::Receiver<ExtractRequest>,
    assets_dir: PathBuf,
) -> Result<()> {
    use persistence::TreeAccess;

    let mut key = Vec::with_capacity(32);
    let mut dummy = default_persisted_download_task();
    let tasks = db.tasks();
    let results = db.results();

    while let Some(ExtractRequest {
        download_task,
        crate_name,
        crate_version,
    }) = r.recv().await
    {
        progress.set_name(format!("🏋️ ‍{}:{}", crate_name, crate_version));
        progress.init(None, Some("files"));

        let mut kt = (crate_name.as_str(), crate_version.as_str(), dummy);
        key.clear();

        persistence::TasksTree::key_to_buf(&kt, &mut key);
        dummy = kt.2;

        let mut task = tasks.update(&key, |t| {
            ({
                t.process = dummy.process.clone();
                t.version = dummy.version.clone()
            })
        })?;

        let downloaded_crate = {
            let crate_version_dir =
                super::iobound::crate_version_dir(&assets_dir, &crate_name, &crate_version);
            super::iobound::download_file_path(
                &download_task.process,
                &download_task.version,
                "crate",
                &crate_version_dir,
            )
        };

        let res: Result<()> = (|| {
            let mut archive = tar::Archive::new(libflate::gzip::Decoder::new(BufReader::new(
                File::open(downloaded_crate)?,
            ))?);
            let mut entries = Vec::new();
            for e in archive.entries()? {
                let e: tar::Entry<_> = e?;
                entries.push(model::TarEntry {
                    path: e.path_bytes().to_vec().into(),
                })
            }

            {
                let insert_item = (
                    crate_name.as_str(),
                    crate_version.as_str(),
                    &task,
                    model::TaskResult::ExplodedCrate {
                        entries: entries.into(),
                    },
                );
                results.insert(&insert_item)?;
            }

            Ok(())
        })();

        task.state = match res {
            Ok(_) => model::TaskState::Complete,
            Err(err) => {
                progress.fail(format!("Failed extract crate: {}", err));
                model::TaskState::AttemptsWithFailure(vec![err.to_string()])
            }
        };
        kt.2 = task;
        tasks.upsert(&kt)?;
        progress.set_name("🏋️‍ idle");
        progress.init(None, None);
    }

    Ok(())
}
