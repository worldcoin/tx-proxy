use clap::Parser;
use tx_proxy::cli;
use dotenv::dotenv;
#[tokio::main]
async fn main() {
    dotenv().ok();
    if let Err(e) = cli::Cli::parse().run().await {
        eprintln!("Fatal Error: {}", e);
        std::process::exit(1);
    }
}
