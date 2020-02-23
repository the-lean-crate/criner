use super::iobound;
use crate::engine::work::iobound::DownloadRequest;
use crate::error::Result;
use crate::persistence::{TasksTree, TreeAccess};
use crate::{model, persistence};

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
    download: &async_std::sync::Sender<iobound::DownloadRequest>,
) -> Result<AsyncResult> {
    let key = (
        version.name.as_ref().into(),
        version.version.as_ref().into(),
        iobound::default_persisted_download_task(),
    );
    let task = tasks.get(TasksTree::key(&key))?.map_or(key.2, |v| v);
    submit_single(task, &mut progress, download, 1, 1, || {
        iobound::DownloadRequest {
            name: version.name.as_ref().into(),
            semver: version.version.as_ref().into(),
            kind: "crate",
            url: format!(
                "https://crates.io/api/v1/crates/{name}/{version}/download",
                name = version.name,
                version = version.version
            ),
        }
    })
    .await;
    Ok(AsyncResult::Done)
}

async fn submit_single(
    task: model::Task<'_>,
    progress: &mut prodash::tree::Item,
    download: &async_std::sync::Sender<iobound::DownloadRequest>,
    step: u32,
    max_step: u32,
    f: impl FnOnce() -> DownloadRequest,
) {
    use model::TaskState::*;
    match task.state {
        NotStarted => {
            progress.init(Some(step), Some("task"));
            progress.set(max_step);
            progress.blocked(None);
            download.send(f()).await;
        }
        AttemptsWithFailure(v) if v.len() < 3 => {
            progress.init(Some(step), Some("task"));
            progress.set(max_step);
            progress.blocked(None);
            progress.info(format!("Retrying task, attempt {}", v.len() + 1));
            download.send(f()).await;
        }
        AttemptsWithFailure(_) => {}
        Complete => {}
    };
}
