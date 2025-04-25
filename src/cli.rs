use crate::{
    client::HttpClient, fanout::FanoutWrite, proxy::ProxyLayer, validation::ValidationLayer,
};
use alloy_rpc_types_engine::JwtSecret;
use clap::Parser;
use eyre::Context as _;
use eyre::{Result, eyre};
use http::{Request, Response, StatusCode};
use hyper::Uri;
use hyper::{server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use jsonrpsee::http_client::HttpBody;
use jsonrpsee::{RpcModule, server::Server};
use metrics_exporter_prometheus::PrometheusBuilder;
use metrics_exporter_prometheus::PrometheusHandle;
use metrics_util::layers::{PrefixLayer, Stack};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{KeyValue, global};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{Resource, propagation::TraceContextPropagator};
use paste::paste;
use rollup_boost::{HealthLayer, LogFormat};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use tokio::net::TcpListener;
use tracing::Level;
use tracing::level_filters::LevelFilter;
use tracing::{error, info};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;

pub const DEFAULT_HTTP_PORT: u16 = 8545;
pub const DEFAULT_METRICS_PORT: u16 = 9090;
pub const DEFAULT_OTLP_URL: &str = "http://localhost:4317";

#[derive(clap::Parser)]
#[clap(about, version, author)]
pub struct Cli {
    #[clap(flatten)]
    pub l2_targets: L2Targets,

    #[clap(flatten)]
    pub builder_targets: BuilderTargets,

    /// The address to bind the HTTP server to.
    #[clap(long, env, default_value_t = IpAddr::V4(Ipv4Addr::LOCALHOST))]
    pub http_addr: IpAddr,

    /// The port to bind the HTTP server to.
    #[clap(long, env, default_value_t = DEFAULT_HTTP_PORT)]
    pub http_port: u16,

    /// Enable Prometheus metrics
    #[arg(long, env, default_value = "false")]
    pub metrics: bool,

    /// Host to run the metrics server on
    #[arg(long, env, default_value_t = IpAddr::V4(Ipv4Addr::UNSPECIFIED))]
    pub metrics_host: IpAddr,

    /// Port to run the metrics server on
    #[arg(long, env, default_value_t = DEFAULT_METRICS_PORT)]
    pub metrics_port: u16,

    // Enable tracing
    #[arg(long, env, default_value = "false")]
    pub tracing: bool,

    /// OTLP endpoint
    #[arg(long, env, default_value = DEFAULT_OTLP_URL)]
    pub otlp_endpoint: Uri,

    /// Log level
    #[arg(long, env, default_value = "info")]
    pub log_level: Level,

    /// Log format
    #[arg(long, env, default_value = "text")]
    pub log_format: LogFormat,

    /// Maximum number of concurrent connections to allow.
    ///
    /// Defaults to 500.
    #[clap(long = "http.max-concurrent-connections", default_value_t = 500)]
    pub max_concurrent_connections: u32,
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("TLS Error: Failed to install default provider");

        self.init_tracing()?;
        self.init_metrics()?;

        let l2_targets = self.l2_targets.build()?;
        let builder_targets = self.builder_targets.build()?;

        let middleware = tower::ServiceBuilder::new()
            .layer(HealthLayer)
            .layer(ValidationLayer::new(builder_targets))
            .layer(ProxyLayer::new(l2_targets));

        let server = Server::builder()
            .set_http_middleware(middleware)
            .max_connections(self.max_concurrent_connections)
            .build(format!("{}:{}", self.http_addr, self.http_port))
            .await?;

        info!(
            target: "tx-proxy::cli",
            url = %server.local_addr()?,
            "Transaction Proxy Server Started"
        );

        let handle = server.start(RpcModule::new(()));

        let stopped_handle = handle.clone();
        let shutdown_handle = handle.clone();

        tokio::select! {
            _ = stopped_handle.stopped() => {
                error!("Server stopped unexpectedly or crashed");
                Err(eyre::eyre!("Server stopped unexpectedly or crashed"))
            }
            _ = tokio::signal::ctrl_c() => {
                error!("Received Ctrl-C, shutting down...");
                shutdown_handle.stop()?;
                Ok(())
            }
        }
    }

    fn init_metrics(&self) -> Result<()> {
        if self.metrics {
            let recorder = PrometheusBuilder::new().build_recorder();
            let handle = recorder.handle();

            Stack::new(recorder)
                .push(PrefixLayer::new("tx-proxy"))
                .install()?;

            // Start the metrics server
            let metrics_addr = format!("{}:{}", self.metrics_host, self.metrics_port);
            let addr: SocketAddr = metrics_addr.parse()?;
            tokio::spawn(init_metrics_server(addr, handle)); // Run the metrics server in a separate task
        }

        Ok(())
    }

    fn init_tracing(&self) -> Result<()> {
        // Be cautious with snake_case and kebab-case here
        let filter_name = "tx-proxy".to_string();

        let global_filter = Targets::new()
            .with_default(LevelFilter::INFO)
            .with_target(&filter_name, LevelFilter::TRACE);

        let registry = tracing_subscriber::registry().with(global_filter);

        let log_filter = Targets::new()
            .with_default(LevelFilter::INFO)
            .with_target(&filter_name, self.log_level);

        // Weird control flow here is required because of type system
        if self.tracing {
            global::set_text_map_propagator(TraceContextPropagator::new());
            let otlp_exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(self.otlp_endpoint.to_string())
                .build()
                .context("Failed to create OTLP exporter")?;
            let provider_builder = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_batch_exporter(otlp_exporter)
                .with_resource(
                    Resource::builder_empty()
                        .with_attributes([
                            KeyValue::new("service.name", env!("CARGO_PKG_NAME")),
                            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                        ])
                        .build(),
                );

            let provider = provider_builder.build();
            let tracer = provider.tracer(env!("CARGO_PKG_NAME"));

            let trace_filter = Targets::new()
                .with_default(LevelFilter::OFF)
                .with_target(&filter_name, LevelFilter::TRACE);

            let registry = registry.with(OpenTelemetryLayer::new(tracer).with_filter(trace_filter));

            match self.log_format {
                LogFormat::Json => {
                    tracing::subscriber::set_global_default(
                        registry.with(
                            tracing_subscriber::fmt::layer()
                                .json()
                                .with_ansi(false)
                                .with_filter(log_filter.clone()),
                        ),
                    )?;
                }
                LogFormat::Text => {
                    tracing::subscriber::set_global_default(
                        registry.with(
                            tracing_subscriber::fmt::layer()
                                .with_ansi(false)
                                .with_filter(log_filter.clone()),
                        ),
                    )?;
                }
            }
        } else {
            match self.log_format {
                LogFormat::Json => {
                    tracing::subscriber::set_global_default(
                        registry.with(
                            tracing_subscriber::fmt::layer()
                                .json()
                                .with_ansi(false)
                                .with_filter(log_filter.clone()),
                        ),
                    )?;
                }
                LogFormat::Text => {
                    tracing::subscriber::set_global_default(
                        registry.with(
                            tracing_subscriber::fmt::layer()
                                .with_ansi(false)
                                .with_filter(log_filter.clone()),
                        ),
                    )?;
                }
            }
        }

        Ok(())
    }
}

pub(crate) async fn init_metrics_server(
    addr: SocketAddr,
    handle: PrometheusHandle,
) -> eyre::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!("Metrics server running on {}", addr);

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let handle = handle.clone(); // Clone the handle for each connection
                tokio::task::spawn(async move {
                    let service = service_fn(move |_req: Request<hyper::body::Incoming>| {
                        let response = match _req.uri().path() {
                            "/metrics" => Response::builder()
                                .header("content-type", "text/plain")
                                .body(HttpBody::from(handle.render()))
                                .unwrap(),
                            _ => Response::builder()
                                .status(StatusCode::NOT_FOUND)
                                .body(HttpBody::empty())
                                .unwrap(),
                        };
                        async { Ok::<_, hyper::Error>(response) }
                    });

                    let io = TokioIo::new(stream);

                    if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                        error!(message = "Error serving metrics connection", error = %err);
                    }
                });
            }
            Err(e) => {
                error!(message = "Error accepting connection", error = %e);
            }
        }
    }
}

macro_rules! define_rpc_args {
    ($(($name:ident, $prefix:ident)),*) => {
        $(
            paste! {
                #[derive(Parser, Debug, Clone, PartialEq, Eq)]
                pub struct $name {
                    /// RPC Server 0
                    #[arg(long, env)]
                    pub [<$prefix _url_0>]: Uri,

                    /// RPC Server 1
                    #[arg(long, env)]
                    pub [<$prefix _url_1>]: Uri,

                    /// RPC Server 2
                    #[arg(long, env)]
                    pub [<$prefix _url_2>]: Uri,

                    /// Hex encoded JWT secret to use for an authenticated RPC server.
                    #[arg(long, env, value_name = "HEX")]
                    pub [<$prefix _jwt_token>]: Option<JwtSecret>,

                    /// Path to a JWT secret to use for an authenticated RPC server.
                    #[arg(long, env, value_name = "PATH")]
                    pub [<$prefix _jwt_path>]: Option<PathBuf>,

                    /// Timeout for http calls in milliseconds
                    #[arg(long, env, default_value_t = 1000)]
                    pub [<$prefix _timeout>]: u64,
                }

                impl $name {
                    fn get_jwt(&self) -> Result<JwtSecret> {
                        if let Some(secret) = &self.[<$prefix _jwt_token>] {
                            Ok(secret.clone())
                        } else if let Some(path) = &self.[<$prefix _jwt_path>] {
                            Ok(JwtSecret::from_file(path)?)
                        } else {
                            Err(eyre!(
                                "No JWT secret provided. Please provide either a hex encoded JWT secret or a path to a file containing the JWT secret."
                            ))
                        }
                    }

                    pub fn build(&self) -> Result<FanoutWrite> {
                        let jwt = self.get_jwt()?;
                        let client_0 = HttpClient::new(self.[<$prefix _url_0>].clone(), jwt.clone(), self.[<$prefix _timeout>],);
                        let client_1 = HttpClient::new(self.[<$prefix _url_1>].clone(), jwt.clone(), self.[<$prefix _timeout>]);
                        let client_2 = HttpClient::new(self.[<$prefix _url_2>].clone(), jwt, self.[<$prefix _timeout>]);
                        Ok(FanoutWrite::new(vec![client_0, client_1, client_2]))
                    }
                }
            }
        )*
    };
}

define_rpc_args!((BuilderTargets, builder), (L2Targets, l2));
