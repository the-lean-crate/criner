use crate::{
    engine::work,
    error::Result,
    model,
    persistence::{Db, Keyed, TreeAccess},
};
use futures::FutureExt;
use std::path::PathBuf;

pub async fn process(
    db: Db,
    mut progress: prodash::tree::Item,
    num_io_processors: u32,
    mut download_progress: prodash::tree::Item,
    tokio: tokio::runtime::Handle,
    assets_dir: PathBuf,
) -> Result<()> {
    let (tx, rx) = async_std::sync::channel(1);
    for idx in 0..num_io_processors {
        // Can only use the pool if the downloader uses a futures-compatible runtime
        // Tokio is its very own thing, and futures requiring it need to run there.
        tokio.spawn(
            work::iobound::processor(
                db.clone(),
                download_progress.add_child(format!("DL {} - idle", idx + 1)),
                rx.clone(),
                assets_dir.clone(),
            )
            .map(|_| ()),
        );
    }

    let versions = db.crate_versions();
    let mut ofs = 0;
    loop {
        let chunk = {
            let tree_iter = versions.tree().iter();
            tree_iter.skip(ofs).take(1000).collect::<Vec<_>>()
        };
        progress.init(Some((ofs + chunk.len()) as u32), Some("crate version"));
        if chunk.is_empty() {
            return Ok(());
        }
        let chunk_len = chunk.len();
        for (idx, res) in chunk.into_iter().enumerate() {
            let (_key, value) = res?;
            progress.set((ofs + idx + 1) as u32);
            let version: model::CrateVersion = value.into();

            progress.blocked(None);
            work::schedule::tasks(
                db.tasks(),
                &version,
                progress.add_child(format!("schedule {}", version.key_string()?)),
                work::schedule::Scheduling::AtLeastOne,
                &tx,
            )
            .await?;
        }
        ofs += chunk_len;
    }
}
