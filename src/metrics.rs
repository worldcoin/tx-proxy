use std::net::SocketAddr;

use http::{Request, Response, StatusCode};
use hyper::{server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use jsonrpsee::http_client::HttpBody;
use metrics_exporter_prometheus::PrometheusHandle;
use tokio::net::TcpListener;
use tracing::{error, info};

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
