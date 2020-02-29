use crate::{error::Result, model, persistence};
use std::io::Read;
use std::{fs::File, io::BufReader, path::PathBuf, time::SystemTime};

pub struct ExtractRequest {
    pub download_task: model::TaskOwned,
    pub crate_name: String,
    pub crate_version: String,
}

pub fn default_persisted_extraction_task() -> model::Task<'static> {
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
    let mut dummy = default_persisted_extraction_task();
    let tasks = db.tasks();
    let results = db.results();

    while let Some(ExtractRequest {
        download_task,
        crate_name,
        crate_version,
    }) = r.recv().await
    {
        progress.set_name(format!("CPU UNZIP+UNTAR {}:{}", crate_name, crate_version));
        progress.init(None, Some("files extracted"));

        let mut kt = (crate_name.as_str(), crate_version.as_str(), dummy);
        key.clear();

        persistence::TasksTree::key_to_buf(&kt, &mut key);
        dummy = kt.2;

        let mut task = tasks.update(&key, |t| {
            t.process = dummy.process.clone();
            t.version = dummy.version.clone();
            t.state = t.state.merged(&model::TaskState::InProgress(None));
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
            let mut meta_data = Vec::new();
            let mut files = Vec::new();
            let mut buf = Vec::new();

            let mut count = 0;
            let mut file_count = 0;
            for e in archive.entries()? {
                count += 1;
                progress.set(count);
                let mut e: tar::Entry<_> = e?;
                let path = e.path().ok();
                meta_data.push(model::TarHeader {
                    path: e.path_bytes().to_vec().into(),
                    size: e.header().size()?,
                    entry_type: e.header().entry_type().as_byte(),
                });

                if let Some(stem_lowercase) = path.and_then(|p| {
                    p.file_stem()
                        .and_then(|stem| stem.to_str().map(str::to_lowercase))
                }) {
                    let interesting_files = ["cargo", "cargo", "readme", "license"];
                    if interesting_files.contains(&stem_lowercase.as_str()) {
                        file_count += 1;
                        buf.clear();
                        e.read_to_end(&mut buf)?;
                        files.push((
                            meta_data
                                .last()
                                .expect("to have pushed one just now")
                                .to_owned(),
                            buf.to_owned().into(),
                        ));
                    }
                }
            }
            progress.info(format!(
                "Recorded {} files and stored {} in full",
                count, file_count
            ));

            {
                let insert_item = (
                    crate_name.as_str(),
                    crate_version.as_str(),
                    &task,
                    model::TaskResult::ExplodedCrate {
                        entries_meta_data: meta_data.into(),
                        selected_entries: files.into(),
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
        progress.set_name("CPU IDLE");
        progress.init(None, None);
    }

    Ok(())
}
