use asset_cli::Cli;
use clap::Parser;

#[tokio::main]
async fn main() {
    if let Err(error) = asset_cli::run(Cli::parse()).await {
        eprintln!("asset: {error}");
        std::process::exit(1);
    }
}
