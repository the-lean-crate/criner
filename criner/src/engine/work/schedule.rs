use crate::{
    engine::{work::cpubound, work::iobound},
    error::Result,
    model, persistence,
    persistence::{TableAccess, TaskTable},
};
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

const MAX_ATTEMPTS_BEFORE_WE_GIVE_UP: usize = 8;

#[derive(Clone, Copy)]
pub enum Scheduling {
    //   /// Considers work done if everything was done. Will block to assure that
    //    All,
    /// Considers the work done if at least one task was scheduled. Will block to wait otherwise.
    AtLeastOne,
    // /// Prefer to never wait for workers to perform a task and instead return without having scheduled anything
    // NeverBlock,
}

pub enum AsyncResult {
    // /// The required scheduling cannot be fulfilled without blocking
    // WouldBlock,
    /// The minimal scheduling requirement was met
    Done,
}

#[allow(clippy::too_many_arguments)]
pub async fn tasks(
    assets_dir: &Path,
    tasks: &persistence::TaskTable,
    krate: &model::CrateVersion,
    mut progress: prodash::tree::Item,
    _mode: Scheduling,
    perform_io: &async_channel::Sender<iobound::DownloadRequest>,
    perform_cpu: &async_channel::Sender<cpubound::ExtractRequest>,
    startup_time: SystemTime,
) -> Result<AsyncResult> {
    use SubmitResult::*;
    let mut key_buf = String::with_capacity(32);
    let io_task = task_or_default(tasks, &mut key_buf, krate, iobound::default_persisted_download_task)?;

    let kind = "crate";
    let submit_result = submit_single(startup_time, io_task, &mut progress, perform_io, 1, 1, || {
        let dummy_task = iobound::default_persisted_download_task();
        let mut task_key = String::new();
        dummy_task.fq_key(&krate.name, &krate.version, &mut task_key);

        iobound::DownloadRequest {
            output_file_path: download_file_path(
                assets_dir,
                &krate.name,
                &krate.version,
                &dummy_task.process,
                &dummy_task.version,
                kind,
            ),
            progress_name: format!("{}:{}", krate.name, krate.version),
            task_key,
            crate_name_and_version: Some((krate.name.clone(), krate.version.clone())),
            kind,
            url: format!(
                "https://static.crates.io/crates/{name}/{name}-{version}.crate",
                name = krate.name,
                version = krate.version
            ),
        }
    })
    .await;

    Ok(match submit_result {
        PermanentFailure | Submitted => AsyncResult::Done,
        Done(download_crate_task) => {
            let cpu_task = task_or_default(tasks, &mut key_buf, krate, cpubound::default_persisted_extraction_task)?;
            submit_single(startup_time, cpu_task, &mut progress, perform_cpu, 2, 2, || {
                cpubound::ExtractRequest {
                    download_task: download_crate_task,
                    crate_name: krate.name.clone(),
                    crate_version: krate.version.clone(),
                }
            })
            .await;
            AsyncResult::Done
        }
    })
}

fn task_or_default(
    tasks: &TaskTable,
    key_buf: &mut String,
    crate_version: &model::CrateVersion,
    make_task: impl FnOnce() -> model::Task,
) -> Result<model::Task> {
    let task = make_task();
    key_buf.clear();
    task.fq_key(&crate_version.name, &crate_version.version, key_buf);
    Ok(tasks.get(key_buf)?.unwrap_or(task))
}

enum SubmitResult {
    Submitted,
    Done(model::Task),
    PermanentFailure,
}

async fn submit_single<R>(
    startup_time: SystemTime,
    task: model::Task,
    progress: &mut prodash::tree::Item,
    channel: &async_channel::Sender<R>,
    step: usize,
    max_step: usize,
    f: impl FnOnce() -> R,
) -> SubmitResult {
    use model::TaskState::*;
    use SubmitResult::*;
    let mut configure = || {
        progress.init(Some(step), Some("task".into()));
        progress.set(max_step);
        progress.blocked("wait for consumer", None);
    };
    match task.state {
        InProgress(_) => {
            if startup_time > task.stored_at {
                configure();
                channel.send(f()).await.unwrap();
            };
            Submitted
        }
        NotStarted => {
            configure();
            channel.send(f()).await.unwrap();
            Submitted
        }
        AttemptsWithFailure(ref v) if v.len() < MAX_ATTEMPTS_BEFORE_WE_GIVE_UP => {
            configure();
            progress.info(format!("Retrying task, attempt {}", v.len() + 1));
            channel.send(f()).await.unwrap();
            Submitted
        }
        AttemptsWithFailure(_) => PermanentFailure,
        Complete => Done(task),
    }
}

fn crate_dir(assets_dir: &Path, crate_name: &str) -> PathBuf {
    // we can safely assume ascii here - otherwise we panic
    let crate_path = match crate_name.len() {
        1 => Path::new("1").join(crate_name),
        2 => Path::new("2").join(crate_name),
        3 => Path::new("3").join(&crate_name[..1]).join(&crate_name[1..]),
        _ => Path::new(&crate_name[..2]).join(&crate_name[2..4]).join(crate_name),
    };
    assets_dir.join(crate_path)
}

pub fn download_file_path(
    assets_dir: &Path,
    crate_name: &str,
    crate_version: &str,
    process: &str,
    version: &str,
    kind: &str,
) -> PathBuf {
    crate_dir(assets_dir, crate_name).join(format!(
        "{crate_version}-{process}{sep}{version}.{kind}",
        process = process,
        sep = crate::persistence::KEY_SEP_CHAR,
        version = version,
        kind = kind,
        crate_version = crate_version
    ))
}
