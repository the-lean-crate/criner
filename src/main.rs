use structopt::StructOpt;

fn main() -> criner::error::Result<()> {
    env_logger::init();
    criner_cli::run_blocking(criner_cli::Parsed::from_args())
}
