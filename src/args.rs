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
    #[structopt(display_order = 0)]
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

        /// The amount of IO-bound processors to run concurrently.
        ///
        /// A way to choose a value is to see which part of the I/O is actually the bottle neck.
        /// Depending on that number, one should experiment with an amount of processors that saturate
        /// either input or output.
        /// Most commonly, these are bound to the input, as it is the network.
        #[structopt(long, alias = "io", default_value = "10")]
        io_bound_processors: u32,

        /// The amount of CPU- and Output-bound processors to run concurrently.
        ///
        /// These will perform a computation followed by flushing its result to disk in the form
        /// of multiple small files.
        /// It's recommended to adjust that number to whatever can saturate the speed of writing to disk,
        /// as these processors will yield when writing, allowing other processors to compute.
        /// Computes are relatively inexpensive compared to the writes.
        #[structopt(long, alias = "cpu-o", default_value = "20")]
        cpu_o_bound_processors: u32,

        /// The amount of CPU-bound processors to run concurrently.
        ///
        /// One can assume that one of these can occupy one core of a CPU.
        /// However, they will not use a lot of IO, nor will they use much memory.
        #[structopt(long, alias = "cpu", default_value = "4")]
        cpu_bound_processors: u32,

        /// Path to the possibly existing crates.io repository clone. If unset, it will be cloned to a temporary spot.
        #[structopt(short = "c", long, name = "REPO")]
        repository: Option<PathBuf>,

        /// The amount of time we can take for the computation. Specified in humantime, like 10s, 5min, or 2h, or '3h 2min 2s'
        #[structopt(long, short = "t")]
        time_limit: Option<humantime::Duration>,

        /// The time between each fetch operation, specified in humantime, like 10s, 5min, or 2h, or '3h 2min 2s'
        #[structopt(long, short = "f", default_value = "60s")]
        fetch_every: humantime::Duration,

        /// If set, the amount of times the fetch stage will run. If set to 0, it will never run.
        #[structopt(long, short = "F")]
        fetch_at_most: Option<usize>,

        /// The time between each processing run, specified in humantime, like 10s, 5min, or 2h, or '3h 2min 2s'
        #[structopt(long, short = "p", default_value = "60s")]
        process_every: humantime::Duration,

        /// If set, the amount of times the process stage will run. If set to 0, they will never run.
        #[structopt(long, short = "P")]
        process_at_most: Option<usize>,

        /// The time between each reporting and processing run, specified in humantime, like 10s, 5min, or 2h, or '3h 2min 2s'
        #[structopt(long, short = "r", default_value = "60s")]
        report_every: humantime::Duration,

        /// If set, the amount of times the reporting stage will run. If set to 0, they will never run.
        #[structopt(long, short = "R")]
        report_at_most: Option<usize>,

        /// If set, the reporting stage will only iterate over crates that match the given standard unix glob.
        ///
        /// moz* would match only crates starting with 'moz' for example.
        #[structopt(long, short = "g")]
        glob: Option<String>,

        /// Path to the possibly existing database. It's used to persist all mining results.
        db_path: PathBuf,
    },
    /// Export all Criner data into a format friendly for exploration via SQL, best viewed with https://sqlitebrowser.org
    ///
    /// Criner stores binary blobs internally and migrates them on the fly, which is optimized for raw performance.
    /// It's also impractical for exploring the data by hand, so the exported data will explode all types into
    /// tables with each column being a field. Foreign key relations are set accordingly to allow joins.
    /// Use this to get an overview of what's available, and possibly contribute a report generator which implements
    /// a query using raw data and writes it into reports.
    #[structopt(display_order = 1)]
    Export {
        /// The path to the source database in sqlite format
        input_db_path: PathBuf,

        /// Path to which to write the exported data. If it exists the operation will fail.
        export_db_path: PathBuf,
    },
    #[cfg(feature = "migration")]
    /// A special purpose command only to be executed in special circumstances
    #[structopt(display_order = 9)]
    Migrate,
}

impl Default for SubCommands {
    fn default() -> Self {
        SubCommands::Mine {
            no_gui: false,
            fps: 3.0,
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
            report_every: std::time::Duration::from_secs(60).into(),
            report_at_most: None,
            db_path: PathBuf::from("criner.db"),
            glob: None,
        }
    }
}
