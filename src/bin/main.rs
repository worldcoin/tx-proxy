use clap::Parser;
use dotenv::dotenv;
use tx_proxy::cli;
#[tokio::main]
async fn main() {
    dotenv().ok();
    if let Err(e) = cli::Cli::parse().run().await {
        eprintln!("Fatal Error: {}", e);
        std::process::exit(1);
    }
}
