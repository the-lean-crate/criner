use clap::Clap;
use std::path::PathBuf;

fn parse_local_time(src: &str) -> Result<time::Time, time::ParseError> {
    time::parse(src, "%R")
}

#[derive(Debug, Clap)]
#[clap(about = "Interact with crates.io from the command-line")]
#[clap(setting = clap::AppSettings::ColoredHelp)]
#[clap(setting = clap::AppSettings::ColorAuto)]
pub struct Args {
    #[clap(subcommand)]
    pub sub: Option<SubCommands>,
}

#[derive(Debug, Clap)]
pub enum SubCommands {
    /// Mine crates.io in an incorruptible and resumable fashion
    #[clap(display_order = 0)]
    #[clap(setting = clap::AppSettings::DisableVersion)]
    Mine {
        /// If set, no gui will be presented. Best with RUST_LOG=info to see basic information.
        #[clap(long)]
        no_gui: bool,

        /// The amount of frames to show per second
        #[clap(long, name = "frames-per-second", default_value = "6.0")]
        fps: f32,

        /// The amount of progress messages to keep in a ring buffer.
        #[clap(short = "s", long, default_value = "100")]
        progress_message_scrollback_buffer_size: usize,

        /// The amount of IO-bound processors to run concurrently.
        ///
        /// A way to choose a value is to see which part of the I/O is actually the bottle neck.
        /// Depending on that number, one should experiment with an amount of processors that saturate
        /// either input or output.
        /// Most commonly, these are bound to the input, as it is the network.
        #[clap(long, alias = "io", value_name = "io", default_value = "10")]
        io_bound_processors: u32,

        /// The amount of CPU- and Output-bound processors to run concurrently.
        ///
        /// These will perform a computation followed by flushing its result to disk in the form
        /// of multiple small files.
        /// It's recommended to adjust that number to whatever can saturate the speed of writing to disk,
        /// as these processors will yield when writing, allowing other processors to compute.
        /// Computes are relatively inexpensive compared to the writes.
        #[clap(long, alias = "cpu-o", value_name = "cpu-o", default_value = "20")]
        cpu_o_bound_processors: u32,

        /// The amount of CPU-bound processors to run concurrently.
        ///
        /// One can assume that one of these can occupy one core of a CPU.
        /// However, they will not use a lot of IO, nor will they use much memory.
        #[clap(long, alias = "cpu", value_name = "cpu", default_value = "4")]
        cpu_bound_processors: u32,

        /// Path to the possibly existing crates.io repository clone. If unset, it will be cloned to a temporary spot.
        #[clap(short = "c", long, name = "REPO")]
        repository: Option<PathBuf>,

        /// The amount of time we can take for the computation. Specified in humantime, like 10s, 5min, or 2h, or '3h 2min 2s'
        #[clap(long, short = "t")]
        time_limit: Option<humantime::Duration>,

        /// The time between each fetch operation, specified in humantime, like 10s, 5min, or 2h, or '3h 2min 2s'
        #[clap(long, short = "f", default_value = "5min")]
        fetch_every: humantime::Duration,

        /// If set, the amount of times the fetch stage will run. If set to 0, it will never run.
        #[clap(long, short = "F")]
        fetch_at_most: Option<usize>,

        /// The time between each processing run, specified in humantime, like 10s, 5min, or 2h, or '3h 2min 2s'
        #[clap(long, short = "p", default_value = "5min")]
        process_every: humantime::Duration,

        /// If set, the amount of times the process stage will run. If set to 0, they will never run.
        #[clap(long, short = "P")]
        process_at_most: Option<usize>,

        /// The time between each reporting and processing run, specified in humantime, like 10s, 5min, or 2h, or '3h 2min 2s'
        #[clap(long, short = "r", default_value = "5min")]
        report_every: humantime::Duration,

        /// If set, the amount of times the reporting stage will run. If set to 0, they will never run.
        #[clap(long, short = "R")]
        report_at_most: Option<usize>,

        /// If set, declare at which local time to download the crates.io database and digest it.
        ///
        /// This job runs every 24h, as the database is updated that often.
        /// If unset, the job starts right away.
        /// Format is HH:MM, e.g. '14:30' for 2:30 pm or 03:15 for quarter past 3 in the morning.
        #[clap(long, short = "d", parse(try_from_str = parse_local_time))]
        download_crates_io_database_every_24_hours_starting_at: Option<time::Time>,

        /// If set, the reporting stage will only iterate over crates that match the given standard unix glob.
        ///
        /// moz* would match only crates starting with 'moz' for example.
        #[clap(long, short = "g")]
        glob: Option<String>,

        /// Path to the possibly existing database. It's used to persist all mining results.
        #[clap(default_value = "criner.db")]
        db_path: PathBuf,
    },
    /// Export all Criner data into a format friendly for exploration via SQL, best viewed with https://sqlitebrowser.org
    ///
    /// Criner stores binary blobs internally and migrates them on the fly, which is optimized for raw performance.
    /// It's also impractical for exploring the data by hand, so the exported data will explode all types into
    /// tables with each column being a field. Foreign key relations are set accordingly to allow joins.
    /// Use this to get an overview of what's available, and possibly contribute a report generator which implements
    /// a query using raw data and writes it into reports.
    #[clap(display_order = 1)]
    #[clap(setting = clap::AppSettings::DisableVersion)]
    Export {
        /// The path to the source database in sqlite format
        input_db_path: PathBuf,

        /// Path to which to write the exported data. If it exists the operation will fail.
        export_db_path: PathBuf,
    },
    #[cfg(feature = "migration")]
    /// A special purpose command only to be executed in special circumstances
    #[clap(display_order = 9)]
    Migrate,
}

impl Default for SubCommands {
    fn default() -> Self {
        SubCommands::Mine {
            no_gui: false,
            fps: 6.0,
            progress_message_scrollback_buffer_size: 100,
            io_bound_processors: 5,
            cpu_bound_processors: 2,
            cpu_o_bound_processors: 10,
            repository: None,
            time_limit: None,
            fetch_every: std::time::Duration::from_secs(60).into(),
            fetch_at_most: None,
            process_every: std::time::Duration::from_secs(60).into(),
            process_at_most: None,
            download_crates_io_database_every_24_hours_starting_at: Some(
                parse_local_time("3:00").expect("valid statically known time"),
            ),
            report_every: std::time::Duration::from_secs(60).into(),
            report_at_most: None,
            db_path: PathBuf::from("criner.db"),
            glob: None,
        }
    }
}
