use std::{ops::Add, path::PathBuf};

mod args;
pub mod error;
pub use args::*;

pub fn run_blocking(args: Parsed) -> criner::error::Result<()> {
    use SubCommands::*;
    let cmd = args.sub.unwrap_or_else(|| SubCommands::Mine {
        no_gui: false,
        fps: 3.0,
        progress_message_scrollback_buffer_size: 100,
        concurrent_downloads: 5,
        repository: None,
        time_limit: None,
        db_path: PathBuf::from("criner.db"),
    });
    match cmd {
        #[cfg(feature = "migration")]
        Migrate => unimplemented!(),
        Mine {
            repository,
            db_path,
            fps,
            time_limit,
            concurrent_downloads,
            no_gui,
            progress_message_scrollback_buffer_size,
        } => criner::run_blocking(
            db_path,
            repository
                .unwrap_or_else(|| std::env::temp_dir().join("criner-crates-io-bare-index.git")),
            time_limit.map(|d| std::time::SystemTime::now().add(*d)),
            concurrent_downloads,
            criner::prodash::TreeOptions {
                message_buffer_capacity: progress_message_scrollback_buffer_size,
                ..criner::prodash::TreeOptions::default()
            }
            .create(),
            if no_gui {
                None
            } else {
                Some(criner::prodash::tui::TuiOptions {
                    title: "Criner".into(),
                    frames_per_second: fps,
                    ..criner::prodash::tui::TuiOptions::default()
                })
            },
        ),
    }
}
