use super::iobound;
use crate::{
    engine::work::cpubound,
    error::Result,
    model, persistence,
    persistence::{TasksTree, TreeAccess},
};

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

pub async fn tasks(
    tasks: persistence::TasksTree<'_>,
    krate: &model::CrateVersion<'_>,
    mut progress: prodash::tree::Item,
    _mode: Scheduling,
    perform_io: &async_std::sync::Sender<iobound::DownloadRequest>,
    perform_cpu: &async_std::sync::Sender<cpubound::ExtractRequest>,
) -> Result<AsyncResult> {
    use SubmitResult::*;
    let io_task = task_or_default(&tasks, krate, iobound::default_persisted_download_task)?;
    Ok(
        match submit_single(io_task, &mut progress, perform_io, 1, 1, || {
            iobound::DownloadRequest {
                crate_name: krate.name.as_ref().into(),
                crate_version: krate.version.as_ref().into(),
                kind: "crate",
                url: format!(
                    "https://crates.io/api/v1/crates/{name}/{version}/download",
                    name = krate.name,
                    version = krate.version
                ),
            }
        })
        .await
        {
            PermanentFailure | Submitted => AsyncResult::Done,
            Done(download_crate_task) => {
                let cpu_task =
                    task_or_default(&tasks, krate, cpubound::default_persisted_extraction_task)?;
                submit_single(cpu_task, &mut progress, perform_cpu, 2, 2, || {
                    cpubound::ExtractRequest {
                        download_task: download_crate_task.into(),
                        crate_name: krate.name.as_ref().into(),
                        crate_version: krate.version.as_ref().into(),
                    }
                })
                .await;
                AsyncResult::Done
            }
        },
    )
}

fn task_or_default<'a, 'b>(
    tasks: &TasksTree<'a>,
    version: &model::CrateVersion<'b>,
    make_task: impl FnOnce() -> model::Task<'static>,
) -> Result<model::Task<'a>> {
    let key = (version.name.as_ref(), version.version.as_ref(), make_task());
    Ok(tasks.get(TasksTree::key(&key))?.unwrap_or(key.2))
}

enum SubmitResult<'a> {
    Submitted,
    Done(model::Task<'a>),
    PermanentFailure,
}

async fn submit_single<'a, R>(
    task: model::Task<'a>,
    progress: &mut prodash::tree::Item,
    download: &async_std::sync::Sender<R>,
    step: u32,
    max_step: u32,
    f: impl FnOnce() -> R,
) -> SubmitResult<'a> {
    use model::TaskState::*;
    use SubmitResult::*;
    let mut configure = || {
        progress.init(Some(step), Some("task"));
        progress.set(max_step);
        progress.blocked(None);
    };
    match task.state {
        NotStarted => {
            configure();
            download.send(f()).await;
            Submitted
        }
        AttemptsWithFailure(ref v) if v.len() < 3 => {
            configure();
            progress.info(format!("Retrying task, attempt {}", v.len() + 1));
            download.send(f()).await;
            Submitted
        }
        AttemptsWithFailure(_) => PermanentFailure,
        Complete => Done(task),
    }
}
