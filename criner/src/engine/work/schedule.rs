use super::iobound;
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
    use model::TaskState::*;
    match task.state {
        NotStarted => {
            progress.init(Some(1), Some("task"));
            progress.set(1);
            progress.blocked(None);
            download
                .send(iobound::DownloadRequest {
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
        }
        AttemptsWithFailure(_) => {}
        Complete => {}
    };
    Ok(AsyncResult::Done)
}
