use crate::persistence::new_key_value_query_old_to_new_filtered;
use crate::{
    engine::{report, work},
    error::Result,
    persistence::{self, TableAccess},
    utils::check,
};
use futures::{task::Spawn, task::SpawnExt, FutureExt};
use rusqlite::NO_PARAMS;
use std::{path::PathBuf, time::SystemTime};

pub async fn generate(
    db: persistence::Db,
    mut progress: prodash::tree::Item,
    assets_dir: PathBuf,
    glob: Option<String>,
    deadline: Option<SystemTime>,
    cpu_o_bound_processors: u32,
    pool: impl Spawn + Clone + Send + 'static + Sync,
) -> Result<()> {
    use report::generic::Generator;
    let krates = db.open_crates()?;
    let chunk_size = 500;
    let output_dir = assets_dir
        .parent()
        .expect("assets directory to be in criner.db")
        .join("reports");
    let num_crates = krates.count() as u32;
    progress.init(Some(num_crates), Some("crates"));

    let (rx_result, tx) = {
        let (tx, rx) = async_std::sync::channel(1);
        let (tx_result, rx_result) =
            async_std::sync::channel((cpu_o_bound_processors * 2) as usize);
        for _ in 0..cpu_o_bound_processors {
            pool.spawn(work::simple::processor(rx.clone(), tx_result.clone()).map(|_| ()))?;
        }
        (rx_result, tx)
    };

    let waste_report_dir = output_dir.join(report::waste::Generator::name());
    async_std::fs::create_dir_all(&waste_report_dir).await?;
    let cache_dir = match glob {
        Some(_) => None,
        None => {
            let cd = waste_report_dir.join("cache");
            async_std::fs::create_dir_all(&cd).await?;
            Some(cd)
        }
    };
    let merge_reports = pool.spawn_with_handle({
        let mut merge_progress = progress.add_child("report aggregator");
        merge_progress.init(Some(num_crates / chunk_size), Some("Reports"));
        report::waste::Generator::merge_reports(
            waste_report_dir.clone(),
            cache_dir.clone(),
            merge_progress,
            rx_result,
        )
        .map(|_| ())
        .boxed()
    })?;
    let mut connection = krates.connection().lock();
    let mut statement = new_key_value_query_old_to_new_filtered(
        persistence::CrateTable::table_name(),
        glob.as_ref().map(|s| s.as_str()),
        &mut *connection,
    )?;
    let mut rows = statement.query(NO_PARAMS)?;
    let mut chunk = Vec::<(String, Vec<u8>)>::with_capacity(chunk_size as usize);
    let mut cid = 0;
    while let Some(r) = rows.next()? {
        chunk.push((r.get(0)?, r.get(1)?));
        if chunk.len() == chunk_size as usize {
            cid += 1;
            check(deadline.clone())?;
            progress.set((cid * chunk_size) as u32);
            progress.blocked("write crate report", None);
            tx.send(
                report::waste::Generator::write_files(
                    db.clone(),
                    waste_report_dir.clone(),
                    cache_dir.clone(),
                    chunk,
                    progress.add_child(""),
                )
                .boxed(),
            )
            .await;
            chunk = Vec::with_capacity(chunk_size as usize);
        }
    }
    drop(tx);
    progress.set(num_crates);
    merge_reports.await;
    progress.done("Generating and merging waste report done");
    Ok(())
}
