use super::iobound;
use crate::{
    engine::work::cpubound,
    error::Result,
    model, persistence,
    persistence::{TasksTree, TreeAccess},
};

pub enum Scheduling {
    //   /// Considers work done if everything was done. Will block to assure that
    //    All,
    /// Considers the work done if at least one task was scheduled. Will block to wait otherwise.
    AtLeastOne,
    //    /// Prefer to never wait for workers to perform a task and instead return without having scheduled anything
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
    version: &model::CrateVersion<'_>,
    mut progress: prodash::tree::Item,
    _mode: Scheduling,
    perform_io: &async_std::sync::Sender<iobound::Request>,
    perform_cpu: &async_std::sync::Sender<cpubound::Request>,
) -> Result<AsyncResult> {
    let downloaded_crate_key = (
        version.name.as_ref().into(),
        version.version.as_ref().into(),
        iobound::default_persisted_download_task(),
    );
    let task = tasks
        .get(TasksTree::key(&downloaded_crate_key))?
        .map_or(downloaded_crate_key.2, |v| v);
    submit_single(task, &mut progress, perform_io, 1, 1, || iobound::Request {
        name: version.name.as_ref().into(),
        semver: version.version.as_ref().into(),
        kind: "crate",
        url: format!(
            "https://crates.io/api/v1/crates/{name}/{version}/download",
            name = version.name,
            version = version.version
        ),
    })
    .await;
    Ok(AsyncResult::Done)
}

enum SubmitResult<'a> {
    Submitted(model::Task<'a>),
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
    match task.state {
        NotStarted => {
            progress.init(Some(step), Some("task"));
            progress.set(max_step);
            progress.blocked(None);
            download.send(f()).await;
            Submitted(task)
        }
        AttemptsWithFailure(ref v) if v.len() < 3 => {
            progress.init(Some(step), Some("task"));
            progress.set(max_step);
            progress.blocked(None);
            progress.info(format!("Retrying task, attempt {}", v.len() + 1));
            download.send(f()).await;
            Submitted(task)
        }
        AttemptsWithFailure(_) => PermanentFailure,
        Complete => Done(task),
    }
}
