use clap::Parser;
use tx_proxy::cli::Cli;
#[tokio::main]
async fn main() {
    init_tls();

    let cli = Cli::parse();

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
