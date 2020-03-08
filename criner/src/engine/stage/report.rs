use crate::persistence::new_key_value_query_old_to_new;
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
    deadline: Option<SystemTime>,
    cpu_o_bound_processors: u32,
    pool: impl Spawn + Clone + Send + 'static + Sync,
) -> Result<()> {
    let krates = db.open_crates()?;
    let chunk_size = 500;
    let output_dir = assets_dir
        .parent()
        .expect("assets directory to be in criner.db")
        .join("reports");
    let waste_report_dir = output_dir.join("waste");
    std::fs::create_dir_all(&waste_report_dir)?;
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

    let merge_reports = pool.spawn_with_handle(
        report::waste::Generator::merge_reports(progress.add_child("report aggregator"), rx_result)
            .map(|_| ())
            .boxed(),
    )?;
    let mut connection = krates.connection().lock();
    let mut statement =
        new_key_value_query_old_to_new(persistence::CrateTable::table_name(), &mut *connection)?;
    let mut rows = statement.query(NO_PARAMS)?;
    let mut chunk = Vec::<(String, Vec<u8>)>::with_capacity(chunk_size);
    let mut cid = 0;
    while let Some(r) = rows.next()? {
        chunk.push((r.get(0)?, r.get(1)?));
        if chunk.len() == chunk_size {
            cid += 1;
            check(deadline.clone())?;
            progress.set((cid * chunk_size) as u32);
            progress.blocked("write crate report", None);
            tx.send(
                report::waste::Generator::write_files(
                    db.clone(),
                    waste_report_dir.clone(),
                    chunk,
                    progress.add_child(""),
                )
                .boxed(),
            )
            .await;
            chunk = Vec::with_capacity(chunk_size);
        }
    }
    drop(tx);
    progress.set(num_crates);
    // TODO: Call function to generate top-level report
    let _report = merge_reports.await;
    progress.done("Generating and merging waste report done");
    Ok(())
}
