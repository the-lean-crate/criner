use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(about = "Interact with crates.io from the command-line")]
#[structopt(settings = &[clap::AppSettings::ColoredHelp, clap::AppSettings::ColorAuto])]
pub struct Parsed {
    #[structopt(subcommand)]
    pub sub: Option<SubCommands>,
}

#[derive(Debug, StructOpt)]
pub enum SubCommands {
    /// Mine crates.io in an incorruptible and resumable fashion
    #[structopt(display_order = 2)]
    Mine {
        /// If set, no gui will be presented. Best with RUST_LOG=info to see basic information.
        #[structopt(long)]
        no_gui: bool,

        /// The amount of frames to show per second
        #[structopt(long, name = "frames-per-second", default_value = "3.0")]
        fps: f32,

        /// The amount of progress messages to keep in a ring buffer.
        #[structopt(long, default_value = "100")]
        progress_message_scrollback_buffer_size: usize,

        /// The amount of downloads to run concurrently
        #[structopt(short = "d", long, name = "COUNT", default_value = "10")]
        concurrent_downloads: u32,

        /// Path to the possibly existing crates.io repository clone. If unset, it will be cloned to a temporary spot.
        #[structopt(short = "r", long, name = "REPO")]
        repository: Option<PathBuf>,

        #[structopt(long, name = "DOWNLOADS")]
        /// If set, all downloads will also be stored in this directory. It will be created if needed.
        ///
        /// For now, it's a mere backup and to make work more visible. All downloads are also stored in the database,
        /// which is the source of truth.
        downloads_directory: Option<PathBuf>,

        /// The amount of time we can take for the computation. Specified in humantime, like 10s, 5min, or 2h, or '3h 2min 2s'
        #[structopt(long, short = "t")]
        time_limit: Option<humantime::Duration>,

        /// Path to the possibly existing database. It's used to persist all mining results.
        db_path: PathBuf,
    },
}
