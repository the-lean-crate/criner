use crate::{engine::stage, error::Result, model, persistence::Db, utils::*};
use futures::{
    future::{Either, FutureExt},
    stream::StreamExt,
    task::{Spawn, SpawnExt},
};
use log::{info, warn};
use prodash::tui::{Event, Line};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

pub struct Context {
    pub db: Db,
    pub progress: prodash::tree::Item,
    pub deadline: Option<SystemTime>,
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
    assets_dir: PathBuf,
    pool: impl Spawn + Clone + Send + 'static + Sync,
    tokio: tokio::runtime::Handle,
) -> Result<()> {
    check(deadline)?;

    let interval_s = 60;
    pool.spawn(
        repeat_every_s(
            interval_s,
            {
                let p = progress.clone();
                move || p.add_child("Fetch Timer")
            },
            deadline,
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
        )
        .map(|_| ()),
    )?;

    let interval_s = 60;
    let processing = repeat_every_s(
        interval_s,
        {
            let p = progress.clone();
            move || p.add_child("Processing Timer")
        },
        deadline,
        {
            let progress = progress.clone();
            let db = db.clone();
            let assets_dir = assets_dir.clone();
            let pool = pool.clone();
            move || {
                stage::tasks::process(
                    db.clone(),
                    progress.add_child("Process Crate Versions"),
                    io_bound_processors,
                    cpu_bound_processors,
                    progress.add_child("Downloads"),
                    tokio.clone(),
                    pool.clone(),
                    assets_dir.clone(),
                )
            }
        },
    );

    let interval_s = 10;
    let report = repeat_every_s(
        interval_s,
        {
            let p = progress.clone();
            move || p.add_child("Report Timer")
        },
        deadline,
        move || {
            stage::report::generate(
                db.clone(),
                progress.add_child("Reports"),
                assets_dir.clone(),
                deadline,
            )
        },
    );
    let (res1, res2) = futures::future::join(processing, report).await;
    res1.and(res2)
}

/// For convenience, run the engine and block until done.
pub fn blocking(
    db: impl AsRef<Path>,
    crates_io_path: impl AsRef<Path>,
    deadline: Option<SystemTime>,
    io_bound_processors: u32,
    cpu_bound_processors: u32,
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
    let pool_size = 1usize + cpu_bound_processors as usize;
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
                        warn!("{}", e);
                    }
                }
                Either::Right((_, work_handle)) => {}
            }

            // Make sure the terminal can reset when the gui is done.
            std::io::stdout().flush()?;
        }
        None => {
            let work_result = futures::executor::block_on(work_handle);
            if let Err(e) = work_result {
                warn!("{}", e);
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
            db.context()
                .iter()
                .next_back()
                .and_then(Result::ok)
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
