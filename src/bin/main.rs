use clap::Parser;
use tx_proxy::cli::Cli;
#[tokio::main]
async fn main() {
    if let Err(e) = Cli::parse().run().await {
        eprintln!("Fatal Error: {}", e);
        std::process::exit(1);
    }
}
