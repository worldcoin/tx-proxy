use eyre::Result;
use jsonrpsee::{RpcModule, server::Server};
use rpc::{BuilderBackend, L2Backend};
use std::net::{IpAddr, Ipv4Addr};

use crate::service::{ProxyLayer, health::HealthLayer, validation::ValidationLayer};
mod rpc;

pub const DEFAULT_HTTP_PORT: u16 = 8545;
#[derive(clap::Parser)]
#[clap(about, version, author)]
pub struct Cli {
    #[clap(flatten)]
    pub l2_backend: L2Backend,

    #[clap(flatten)]
    pub builder_backend: BuilderBackend,

    /// The address to bind the HTTP server to.
    #[clap(long = "http.addr", default_value_t = IpAddr::V4(Ipv4Addr::LOCALHOST))]
    pub http_addr: IpAddr,

    /// The port to bind the HTTP server to.
    #[clap(long = "http.port", default_value_t = DEFAULT_HTTP_PORT)]
    pub http_port: u16,

    /// Maximum number of connections to allow.
    #[clap(long = "http.max_connections", default_value_t = 100)]
    pub max_connections: u32,
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        let l2_backend = self.l2_backend.build_backend()?;
        let builder_backend = self.builder_backend.build_backend()?;

        let middleware = tower::ServiceBuilder::new()
            .layer(HealthLayer)
            .layer(ValidationLayer::new(builder_backend))
            .layer(ProxyLayer::new(l2_backend));

        let server = Server::builder()
            .set_http_middleware(middleware)
            .max_connections(self.max_connections)
            .build(format!("{}:{}", self.http_addr, self.http_port))
            .await?;

        let module: RpcModule<()> = RpcModule::new(());
        let handle = server.start(module);

        let stopped_handle = handle.clone();
        let shutdown_handle = handle.clone();

        tokio::select! {
            _ = stopped_handle.stopped() => {
                Err(eyre::eyre!("Server stopped unexpectedly or crashed"))
            }
            _ = tokio::signal::ctrl_c() => {
                println!("Received Ctrl-C, shutting down...");
                shutdown_handle.stop()?;
                Ok(())
            }
        }
    }
}
