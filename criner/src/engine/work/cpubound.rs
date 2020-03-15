use crate::engine::report::waste::{tar_path_to_utf8_str, CargoConfig};
use crate::{error::Result, model, persistence, Error};
use async_trait::async_trait;
use std::io::Seek;
use std::{fs::File, io::BufReader, io::Read, path::PathBuf, time::SystemTime};

struct ProcessingState {
    downloaded_crate: PathBuf,
    key: String,
}
pub struct Agent {
    asset_dir: PathBuf,
    results: persistence::TaskResultTable,
    state: Option<ProcessingState>,
    standard_bin_path: globset::GlobMatcher,
}

impl Agent {
    pub fn new(asset_dir: PathBuf, db: &persistence::Db) -> Result<Agent> {
        let results = db.open_results()?;
        Ok(Agent {
            asset_dir,
            results,
            state: None,
            standard_bin_path: globset::Glob::new("src/bin/*.rs")
                .expect("valid statically known glob")
                .compile_matcher(),
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
        progress: &mut prodash::tree::Item,
    ) -> Result<(model::Task, String)> {
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
                    let crate_dir = super::iobound::crate_dir(&self.asset_dir, &crate_name);
                    super::iobound::download_file_path(
                        &crate_dir,
                        &crate_version,
                        &download_task.process,
                        &download_task.version,
                        "crate",
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
        extract_crate(
            &self.results,
            &key,
            progress,
            downloaded_crate,
            &self.standard_bin_path,
        )
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

fn extract_crate(
    results: &persistence::TaskResultTable,
    key: &str,
    progress: &mut prodash::tree::Item,
    downloaded_crate: PathBuf,
    standard_bin_path: &globset::GlobMatcher,
) -> Result<()> {
    use persistence::TableAccess;
    let mut archive = tar::Archive::new(libflate::gzip::Decoder::new(BufReader::new(File::open(
        downloaded_crate,
    )?))?);

    let mut buf = Vec::new();
    let mut interesting_paths = vec!["Cargo.toml".to_string(), "Cargo.lock".into()];
    let mut files = Vec::new();
    for (eid, e) in archive.entries()?.enumerate() {
        progress.set(eid as u32);
        let mut e: tar::Entry<_> = e?;
        if tar_path_to_utf8_str(e.path_bytes().as_ref()) == "Cargo.toml" {
            e.read_to_end(&mut buf)?;
            let config = CargoConfig::from(buf.as_slice());
            interesting_paths.push(config.actual_or_expected_build_script_path().to_owned());
            interesting_paths.push(config.lib_path().to_owned());
            interesting_paths.extend(config.bin_paths().into_iter().map(|s| s.to_owned()));
            break;
        }
    }

    let mut archive = tar::Archive::new(libflate::gzip::Decoder::new(BufReader::new({
        let mut file = archive.into_inner().into_inner();
        file.seek(std::io::SeekFrom::Start(0))?;
        file
    }))?);

    let mut meta_data = Vec::new();
    let mut meta_count = 0;
    let mut file_count = 0;
    for e in archive.entries()? {
        meta_count += 1;
        progress.set(meta_count);
        let mut e: tar::Entry<_> = e?;
        meta_data.push(model::TarHeader {
            path: e.path_bytes().to_vec(),
            size: e.header().size()?,
            entry_type: e.header().entry_type().as_byte(),
        });

        if interesting_paths
            .iter()
            .any(|p| p == tar_path_to_utf8_str(e.path_bytes().as_ref()))
            || standard_bin_path.is_match(tar_path_to_utf8_str(e.path_bytes().as_ref()))
        {
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
    progress.info(format!(
        "Recorded {} files and stored {} in full",
        meta_count, file_count
    ));

    let task_result = model::TaskResult::ExplodedCrate {
        entries_meta_data: meta_data.into(),
        selected_entries: files.into(),
    };
    results.insert(progress, &key, &task_result)?;

    Ok(())
}
