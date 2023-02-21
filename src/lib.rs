use std::ops::Add;

mod args;
pub mod error;
pub use args::*;

pub fn run_blocking(args: Args) -> criner::error::Result<()> {
    use SubCommands::*;
    let cmd = args.sub.unwrap_or_default();
    match cmd {
        #[cfg(feature = "migration")]
        Migrate => criner::migration::migrate("./criner.db"),
        Export {
            input_db_path,
            export_db_path,
        } => criner::export::run_blocking(input_db_path, export_db_path),
        Mine {
            repository,
            db_path,
            fps,
            time_limit,
            io_bound_processors,
            cpu_bound_processors,
            cpu_o_bound_processors,
            no_gui,
            no_db_download,
            progress_message_scrollback_buffer_size,
            fetch_every,
            fetch_at_most,
            process_at_most,
            process_every,
            download_crates_io_database_every_24_hours_starting_at,
            report_every,
            report_at_most,
            glob,
        } => criner::run::blocking(
            db_path,
            repository.unwrap_or_else(|| std::env::temp_dir().join("criner-crates-io-bare-index.git")),
            time_limit.map(|d| std::time::SystemTime::now().add(*d)),
            io_bound_processors,
            cpu_bound_processors,
            cpu_o_bound_processors,
            !no_db_download,
            criner::run::StageRunSettings {
                every: fetch_every.into(),
                at_most: fetch_at_most,
            },
            criner::run::StageRunSettings {
                every: process_every.into(),
                at_most: process_at_most,
            },
            criner::run::GlobStageRunSettings {
                run: criner::run::StageRunSettings {
                    every: report_every.into(),
                    at_most: report_at_most,
                },
                glob,
            },
            download_crates_io_database_every_24_hours_starting_at,
            criner::prodash::tree::root::Options {
                message_buffer_capacity: progress_message_scrollback_buffer_size,
                ..criner::prodash::tree::root::Options::default()
            }
            .create().into(),
            if no_gui {
                None
            } else {
                Some(criner::prodash::render::tui::Options {
                    title: "Criner".into(),
                    frames_per_second: fps,
                    recompute_column_width_every_nth_frame: Option::from(fps as usize),
                    ..criner::prodash::render::tui::Options::default()
                })
            },
        ),
    }
}
