use structopt::StructOpt;

fn main() -> criner::error::Result<()> {
    let args = criner_cli::Parsed::from_args();
    if let Some(criner_cli::SubCommands::Mine { no_gui, .. }) = args.sub {
        if no_gui {
            env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
        }
    } else {
        env_logger::init();
    }
    criner_cli::run_blocking(args)
}
