use crate::{engine::stage, error::Result, model, persistence::Db, utils::*};
use futures::{
    future::{Either, FutureExt},
    stream::StreamExt,
    task::{Spawn, SpawnExt},
};
use futures_timer::Delay;
use log::{info, warn};
use prodash::tui::{Event, Line};
use std::{
    convert::TryFrom,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

pub struct StageRunSettings {
    /// Wait for the given duration after the stage ran
    pub every: Duration,
    /// If None, run the stage indefinitely. Otherwise run it the given amount of times. Some(0) disables the stage.
    pub at_most: Option<usize>,
}

/// Like `StageRunSettings`, but also provides a glob pattern
pub struct GlobStageRunSettings {
    pub glob: Option<String>,
    pub run: StageRunSettings,
}

/// Runs the statistics and mining engine.
/// May run for a long time unless a deadline is specified.
/// Even though timeouts can be achieved from outside of the future, knowing the deadline may be used
/// by the engine to manage its time even more efficiently.
pub async fn non_blocking(
    db: Db,
    crates_io_path: PathBuf,
    deadline: Option<SystemTime>,
    progress: prodash::Tree,
    io_bound_processors: u32,
    cpu_bound_processors: u32,
    cpu_o_bound_processors: u32,
    fetch_settings: StageRunSettings,
    process_settings: StageRunSettings,
    report_settings: GlobStageRunSettings,
    download_crates_io_database_every_24_hours_starting_at: Option<time::Time>,
    assets_dir: PathBuf,
    pool: impl Spawn + Clone + Send + 'static + Sync,
    tokio: tokio::runtime::Handle,
) -> Result<()> {
    check(deadline)?;
    let startup_time = SystemTime::now();

    let wait_for = wait_duration_until(download_crates_io_database_every_24_hours_starting_at);

    let db_download_handle = pool.spawn_with_handle(repeat_every_s(
        24 * 60 * 60,
        {
            let p = progress.clone();
            move || p.add_child("Fetch Timer")
        },
        deadline,
        None,
        { move || Delay::new(wait_for).map(|_| Ok(())) },
    ))?;

    let run = fetch_settings;
    let fetch_handle = pool.spawn_with_handle(repeat_every_s(
        run.every.as_secs() as u32,
        {
            let p = progress.clone();
            move || p.add_child("Fetch Timer")
        },
        deadline,
        run.at_most,
        {
            let db = db.clone();
            let progress = progress.clone();
            let pool = pool.clone();
            move || {
                stage::changes::fetch(
                    crates_io_path.clone(),
                    pool.clone(),
                    db.clone(),
                    progress.add_child("crates.io refresh"),
                    deadline,
                )
            }
        },
    ))?;

    let stage = process_settings;
    let processing_handle = pool.spawn_with_handle(repeat_every_s(
        stage.every.as_secs() as u32,
        {
            let p = progress.clone();
            move || p.add_child("Processing Timer")
        },
        deadline,
        stage.at_most,
        {
            let progress = progress.clone();
            let db = db.clone();
            let assets_dir = assets_dir.clone();
            let pool = pool.clone();
            let tokio = tokio.clone();
            move || {
                stage::processing::process(
                    db.clone(),
                    progress.add_child("Process Crate Versions"),
                    io_bound_processors,
                    cpu_bound_processors,
                    progress.add_child("Downloads"),
                    tokio.clone(),
                    pool.clone(),
                    assets_dir.clone(),
                    startup_time,
                )
            }
        },
    ))?;

    let stage = report_settings;
    repeat_every_s(
        stage.run.every.as_secs() as u32,
        {
            let p = progress.clone();
            move || p.add_child("Reporting Timer")
        },
        deadline,
        stage.run.at_most,
        {
            let progress = progress.clone();
            let db = db.clone();
            let assets_dir = assets_dir.clone();
            let pool = pool.clone();
            let glob = stage.glob.clone();
            move || {
                stage::report::generate(
                    db.clone(),
                    progress.add_child("Reports"),
                    assets_dir.clone(),
                    glob.clone(),
                    deadline,
                    cpu_o_bound_processors,
                    pool.clone(),
                )
            }
        },
    )
    .await?;

    fetch_handle.await?;
    db_download_handle.await?;
    processing_handle.await
}

fn wait_duration_until(time: Option<time::Time>) -> Duration {
    time.map(|t| {
        let now = time::OffsetDateTime::now_local();
        let desired = now.date().with_time(t).assume_offset(now.offset());
        if desired > now {
            desired - now
        } else {
            desired
                .date()
                .next_day()
                .with_time(t)
                .assume_offset(now.offset())
                - now
        }
    })
    .and_then(|d| Duration::try_from(d).ok())
    .unwrap_or_default()
}

/// For convenience, run the engine and block until done.
pub fn blocking(
    db: impl AsRef<Path>,
    crates_io_path: impl AsRef<Path>,
    deadline: Option<SystemTime>,
    io_bound_processors: u32,
    cpu_bound_processors: u32,
    cpu_o_bound_processors: u32,
    fetch_settings: StageRunSettings,
    process_settings: StageRunSettings,
    report_settings: GlobStageRunSettings,
    download_crates_io_database_every_24_hours_starting_at: Option<time::Time>,
    root: prodash::Tree,
    gui: Option<prodash::tui::TuiOptions>,
) -> Result<()> {
    // required for request
    let tokio_rt = tokio::runtime::Builder::new()
        .enable_all()
        .core_threads(1)
        .max_threads(2) // needs to be two or nothing happens
        .threaded_scheduler()
        .build()?;
    let start_of_computation = SystemTime::now();
    // NOTE: pool should be big enough to hold all possible blocking tasks running in parallel, +1 for
    // additional non-blocking tasks.
    // The main thread is expected to pool non-blocking tasks.
    // I admit I don't fully understand why multi-pool setups aren't making progress… . So just one pool for now.
    let how_much_slower_writes_are_compared_to_computation = 4;
    let pool_size = 1usize
        + cpu_bound_processors
            .max(cpu_o_bound_processors / how_much_slower_writes_are_compared_to_computation)
            as usize;
    let task_pool = futures::executor::ThreadPool::builder()
        .pool_size(pool_size)
        .create()?;
    let assets_dir = db.as_ref().join("assets");
    let db = Db::open(db)?;
    std::fs::create_dir_all(&assets_dir)?;

    // dropping the work handle will stop (non-blocking) futures
    let work_handle = non_blocking(
        db.clone(),
        crates_io_path.as_ref().into(),
        deadline,
        root.clone(),
        io_bound_processors,
        cpu_bound_processors,
        cpu_o_bound_processors,
        fetch_settings,
        process_settings,
        report_settings,
        download_crates_io_database_every_24_hours_starting_at,
        assets_dir,
        task_pool.clone(),
        tokio_rt.handle().clone(),
    );

    match gui {
        Some(gui_options) => {
            let (gui, abort_handle) = futures::future::abortable(prodash::tui::render_with_input(
                root,
                gui_options,
                context_stream(&db, start_of_computation),
            )?);
            let gui = task_pool.spawn_with_handle(gui)?;

            let either = futures::executor::block_on(futures::future::select(
                work_handle.boxed_local(),
                gui.boxed_local(),
            ));
            match either {
                Either::Left((work_result, gui)) => {
                    abort_handle.abort();
                    futures::executor::block_on(gui).ok();
                    if let Err(e) = work_result {
                        warn!("work processor failed: {}", e);
                    }
                }
                Either::Right((_, _work_handle)) => {}
            }
        }
        None => {
            let work_result = futures::executor::block_on(work_handle);
            if let Err(e) = work_result {
                warn!("work processor failed: {}", e);
            }
        }
    };

    // at this point, we forget all currently running computation, and since it's in the local thread, it's all
    // destroyed/dropped properly.
    info!("{}", wallclock(start_of_computation));
    Ok(())
}

fn wallclock(since: SystemTime) -> String {
    format!(
        "Wallclock elapsed: {}",
        humantime::format_duration(SystemTime::now().duration_since(since).unwrap_or_default())
    )
}

fn context_stream(db: &Db, start_of_computation: SystemTime) -> impl futures::Stream<Item = Event> {
    prodash::tui::ticker(Duration::from_secs(1)).map({
        let db = db.clone();
        move |_| {
            db.open_context()
                .ok()
                .and_then(|c| c.most_recent().ok())
                .flatten()
                .map(|(_, c): (_, model::Context)| {
                    let lines = vec![
                        Line::Text(wallclock(start_of_computation)),
                        Line::Title("Durations".into()),
                        Line::Text(format!(
                            "fetch-crate-versions: {:?}",
                            c.durations.fetch_crate_versions
                        )),
                        Line::Title("Counts".into()),
                        Line::Text(format!("crate-versions: {}", c.counts.crate_versions)),
                        Line::Text(format!("        crates: {}", c.counts.crates)),
                    ];
                    Event::SetInformation(lines)
                })
                .unwrap_or(Event::Tick)
        }
    })
}
