use crate::{
    engine::stage,
    error::{Error, Result},
    model,
    persistence::Db,
    utils::*,
};
use futures_util::{
    future::{Either, FutureExt},
    stream::StreamExt,
};
use log::{info, warn};
use prodash::tui::{Event, Line};
use std::{
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
    interrupt_control: InterruptControlEvents,
    fetch_settings: StageRunSettings,
    process_settings: StageRunSettings,
    report_settings: GlobStageRunSettings,
    download_crates_io_database_every_24_hours_starting_at: Option<time::Time>,
    assets_dir: PathBuf,
) -> Result<()> {
    check(deadline)?;
    let startup_time = SystemTime::now();

    let db_download_handle = crate::smol::Task::spawn(repeat_daily_at(
        download_crates_io_database_every_24_hours_starting_at,
        {
            let p = progress.clone();
            move || p.add_child("Crates.io DB Digest")
        },
        deadline,
        {
            let db = db.clone();
            let assets_dir = assets_dir.clone();
            let progress = progress.clone();
            move || {
                stage::db_download::schedule(
                    db.clone(),
                    assets_dir.clone(),
                    progress.add_child("fetching crates-io db"),
                    startup_time,
                )
            }
        },
    ));

    let run = fetch_settings;
    let fetch_handle = crate::smol::Task::spawn(repeat_every_s(
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
            move || {
                stage::changes::fetch(
                    crates_io_path.clone(),
                    db.clone(),
                    progress.add_child("crates.io refresh"),
                    deadline,
                )
            }
        },
    ));

    let stage = process_settings;
    let processing_handle = crate::smol::Task::spawn(repeat_every_s(
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
            move || {
                stage::processing::process(
                    db.clone(),
                    progress.add_child("Process Crate Versions"),
                    io_bound_processors,
                    cpu_bound_processors,
                    progress.add_child("Downloads"),
                    assets_dir.clone(),
                    startup_time,
                )
            }
        },
    ));

    let stage = report_settings;
    let report_handle = crate::smol::Task::spawn(repeat_every_s(
        stage.run.every.as_secs() as u32,
        {
            let p = progress.clone();
            move || p.add_child("Reporting Timer")
        },
        deadline,
        stage.run.at_most,
        {
            move || {
                let progress = progress.clone();
                let db = db.clone();
                let assets_dir = assets_dir.clone();
                let glob = stage.glob.clone();
                let interrupt_control = interrupt_control.clone();
                async move {
                    let ctrl = interrupt_control;
                    ctrl.send(Interruptible::Deferred).await.ok(); // there might be no TUI
                    let res = stage::report::generate(
                        db.clone(),
                        progress.add_child("Reports"),
                        assets_dir.clone(),
                        glob.clone(),
                        deadline,
                        cpu_o_bound_processors,
                    )
                    .await;
                    ctrl.send(Interruptible::Instantly).await.ok(); // there might be no TUI
                    res
                }
            }
        },
    ));

    fetch_handle.await?;
    db_download_handle.await?;
    report_handle.await?;
    processing_handle.await
}

pub enum Interruptible {
    Instantly,
    Deferred,
}

pub type InterruptControlEvents = async_channel::Sender<Interruptible>;

impl From<Interruptible> for prodash::tui::Event {
    fn from(v: Interruptible) -> Self {
        match v {
            Interruptible::Instantly => Event::SetInterruptMode(prodash::tui::Interrupt::Instantly),
            Interruptible::Deferred => Event::SetInterruptMode(prodash::tui::Interrupt::Deferred),
        }
    }
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
    gui: Option<prodash::tui::Options>,
) -> Result<()> {
    let start_of_computation = SystemTime::now();
    let assets_dir = db.as_ref().join("assets");
    let db = Db::open(db)?;
    std::fs::create_dir_all(&assets_dir)?;
    let (interrupt_control_sink, interrupt_control_stream) = async_channel::bounded::<Interruptible>(1);

    // dropping the work handle will stop (non-blocking) futures
    let work_handle = non_blocking(
        db.clone(),
        crates_io_path.as_ref().into(),
        deadline,
        root.clone(),
        io_bound_processors,
        cpu_bound_processors,
        cpu_o_bound_processors,
        interrupt_control_sink,
        fetch_settings,
        process_settings,
        report_settings,
        download_crates_io_database_every_24_hours_starting_at,
        assets_dir,
    );

    match gui {
        Some(gui_options) => {
            let gui = crate::smol::Task::spawn(prodash::tui::render_with_input(
                std::io::stdout(),
                root,
                gui_options,
                futures_util::stream::select(
                    context_stream(&db, start_of_computation),
                    interrupt_control_stream.map(|v| Event::from(v)),
                ),
            )?);

            let either = futures_lite::future::block_on(futures_util::future::select(
                handle_ctrl_c_and_sigterm(work_handle.boxed_local()).boxed_local(),
                gui,
            ));
            match either {
                Either::Left((work_result, gui)) => {
                    futures_lite::future::block_on(gui.cancel());
                    if let Err(e) = work_result? {
                        warn!("work processor failed: {}", e);
                    }
                }
                Either::Right((_, _work_handle)) => {}
            }
        }
        None => {
            drop(interrupt_control_stream);
            let work_result = futures_lite::future::block_on(handle_ctrl_c_and_sigterm(work_handle.boxed_local()));
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

fn context_stream(db: &Db, start_of_computation: SystemTime) -> impl futures_util::stream::Stream<Item = Event> {
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
                        Line::Text(format!("fetch-crate-versions: {:?}", c.durations.fetch_crate_versions)),
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
