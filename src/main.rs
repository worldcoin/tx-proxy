use clap::Parser;

mod cli;
mod client;
mod service;
mod types;

#[tokio::main]
async fn main() {
    let cli = cli::Cli::parse();
    if let Err(e) = cli.run().await {
        eprintln!("Fatal Error: {}", e);
        std::process::exit(1);
    }
}
