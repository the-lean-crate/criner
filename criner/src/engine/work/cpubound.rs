use crate::{
    engine::work::generic::Processor, error::Result, model, model::Task, persistence, Error,
};
use async_trait::async_trait;
use prodash::tree::Item;
use std::{fs::File, io::BufReader, io::Read, path::PathBuf, time::SystemTime};

struct ProcessingState {
    downloaded_crate: PathBuf,
    key: String,
}
pub struct Agent {
    asset_dir: PathBuf,
    results: persistence::TaskResultTree,
    state: Option<ProcessingState>,
}

impl Agent {
    pub fn new(asset_dir: PathBuf, db: &persistence::Db) -> Result<Agent> {
        let results = db.open_results()?;
        Ok(Agent {
            asset_dir,
            results,
            state: None,
        })
    }
}

#[async_trait]
impl crate::engine::work::generic::Processor for Agent {
    type Item = ExtractRequest;

    fn set(
        &mut self,
        request: Self::Item,
        out_key: &mut String,
        progress: &mut Item,
    ) -> Result<(Task, String)> {
        progress.init(None, Some("files extracted"));
        match request {
            ExtractRequest {
                download_task,
                crate_name,
                crate_version,
            } => {
                let progress_info = format!("CPU UNZIP+UNTAR {}:{}", crate_name, crate_version);
                let dummy_task = default_persisted_extraction_task();
                dummy_task.fq_key(&crate_name, &crate_version, out_key);

                let downloaded_crate = {
                    let crate_version_dir = super::iobound::crate_version_dir(
                        &self.asset_dir,
                        &crate_name,
                        &crate_version,
                    );
                    super::iobound::download_file_path(
                        &download_task.process,
                        &download_task.version,
                        "crate",
                        &crate_version_dir,
                    )
                };
                let dummy_result = model::TaskResult::ExplodedCrate {
                    entries_meta_data: vec![],
                    selected_entries: vec![],
                };

                let mut key = String::with_capacity(out_key.len() * 2);
                dummy_result.fq_key(&crate_name, &crate_version, &dummy_task, &mut key);

                self.state = Some(ProcessingState {
                    downloaded_crate,
                    key,
                });
                Ok((dummy_task, progress_info))
            }
        }
    }

    fn idle_message(&self) -> String {
        "CPU IDLE".into()
    }

    async fn process(
        &mut self,
        progress: &mut prodash::tree::Item,
    ) -> std::result::Result<(), (Error, String)> {
        let ProcessingState {
            downloaded_crate,
            key,
        } = self.state.take().expect("state to be set");
        extract_crate(&self.results, &key, progress, downloaded_crate)
            .map_err(|err| (err, "Failed to extract crate".into()))
    }
}

pub struct ExtractRequest {
    pub download_task: model::Task,
    pub crate_name: String,
    pub crate_version: String,
}

pub fn default_persisted_extraction_task() -> model::Task {
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

    let mut key = String::with_capacity(32);
    let dummy_task = default_persisted_extraction_task();
    let dummy_result = model::TaskResult::ExplodedCrate {
        entries_meta_data: vec![],
        selected_entries: vec![],
    };
    let tasks = db.open_tasks()?;
    let results = db.open_results()?;

    while let Some(ExtractRequest {
        download_task,
        crate_name,
        crate_version,
    }) = r.recv().await
    {
        progress.set_name(format!("CPU UNZIP+UNTAR {}:{}", crate_name, crate_version));
        progress.init(None, Some("files extracted"));

        key.clear();
        dummy_task.fq_key(&crate_name, &crate_version, &mut key);

        let mut task = tasks.update(&key, |mut t| {
            t.process = dummy_task.process.clone();
            t.version = dummy_task.version.clone();
            t.state.merge_with(&model::TaskState::InProgress(None));
            t
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

        progress.blocked(None);
        key.clear();
        dummy_result.fq_key(&crate_name, &crate_version, &task, &mut key);
        let res = extract_crate(&results, &key, &mut progress, downloaded_crate);

        task.state = match res {
            Ok(_) => model::TaskState::Complete,
            Err(err) => {
                progress.fail(format!("Failed extract crate: {}", err));
                model::TaskState::AttemptsWithFailure(vec![err.to_string()])
            }
        };

        key.clear();
        task.fq_key(&crate_name, &crate_version, &mut key);
        tasks.upsert(&key, &task)?;

        progress.set_name("CPU IDLE");
        progress.init(None, None);
    }

    Ok(())
}

fn extract_crate(
    results: &persistence::TaskResultTree,
    key: &str,
    progress: &mut prodash::tree::Item,
    downloaded_crate: PathBuf,
) -> Result<()> {
    use persistence::TreeAccess;
    let mut archive = tar::Archive::new(libflate::gzip::Decoder::new(BufReader::new(File::open(
        downloaded_crate,
    )?))?);
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
            path: e.path_bytes().to_vec(),
            size: e.header().size()?,
            entry_type: e.header().entry_type().as_byte(),
        });

        if let Some(stem_lowercase) = path.and_then(|p| {
            p.file_stem()
                .and_then(|stem| stem.to_str().map(str::to_lowercase))
        }) {
            let interesting_files = ["cargo", "cargo", "readme", "license", "build"];
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

    let task_result = model::TaskResult::ExplodedCrate {
        entries_meta_data: meta_data.into(),
        selected_entries: files.into(),
    };
    results.insert(&key, &task_result)?;

    Ok(())
}
