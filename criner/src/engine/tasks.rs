use crate::{
    engine::work,
    error::Result,
    model,
    persistence::{Db, Keyed, TreeAccess},
};

pub async fn process(
    db: Db,
    mut progress: prodash::tree::Item,
    tx: async_std::sync::Sender<work::iobound::DownloadRequest>,
) -> Result<()> {
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
