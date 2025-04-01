#![cfg_attr(not(test), warn(unused_crate_dependencies))]
use clap::Parser;

mod cli;
mod client;
mod service;
mod utils;

// TODO: Tracing/Telemetry
#[tokio::main]
async fn main() {
    init_tls();

    let cli = cli::Cli::parse();
    if let Err(e) = cli.run().await {
        eprintln!("Fatal Error: {}", e);
        std::process::exit(1);
    }
}

fn init_tls() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("TLS Error: Failed to install default provider");
}
