use std::time::Duration;

use crate::rpc::{RpcRequest, RpcResponse, parse_response_payload};
use alloy_rpc_types_engine::JwtSecret;
use http::Uri;
use http_body_util::BodyExt;
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use jsonrpsee::{core::BoxError, http_client::HttpBody};
use opentelemetry::trace::SpanKind;
use rollup_boost::{AuthClientLayer, AuthClientService};
use tower::{
    Service, ServiceBuilder, ServiceExt,
    timeout::{Timeout, TimeoutLayer},
};
use tower_http::decompression::{Decompression, DecompressionLayer};
use tracing::{debug, instrument};

pub type HttpClientService =
    Timeout<Decompression<AuthClientService<Client<HttpsConnector<HttpConnector>, HttpBody>>>>;

#[derive(Clone, Debug)]
pub struct HttpClient {
    client: HttpClientService,
    url: Uri,
}

fn sanitized_target(url: &Uri) -> String {
    match (url.host(), url.port_u16()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_owned(),
        (None, _) => "<unknown>".to_owned(),
    }
}

impl HttpClient {
    pub fn new(url: Uri, secret: JwtSecret, timeout: u64) -> Self {
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .expect("no native root CA certificates found")
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();

        let client_builder = Client::builder(TokioExecutor::new());
        let client = ServiceBuilder::new()
            .layer(TimeoutLayer::new(Duration::from_millis(timeout)))
            .layer(DecompressionLayer::new())
            .layer(AuthClientLayer::new(secret))
            .service(client_builder.build(connector));

        Self { client, url }
    }

    /// Returns a credential-safe target identifier for logs.
    pub(crate) fn log_target(&self) -> String {
        sanitized_target(&self.url)
    }

    #[instrument(
        skip(self, req),
        target = "tx-proxy::http::forward",
        fields(otel.kind = ?SpanKind::Client)
    )]
    pub async fn forward(&mut self, req: RpcRequest) -> Result<RpcResponse<HttpBody>, BoxError> {
        debug!("forwarding {}", req.method);
        let mut req: http::Request<HttpBody> = req.into();
        *req.uri_mut() = self.url.clone();

        let res = self.client.ready().await?.call(req).await?;

        let (parts, body) = res.into_parts();
        let body_bytes = body.collect().await?.to_bytes().to_vec();
        let payload = parse_response_payload(&body_bytes)?;
        let response = http::Response::from_parts(parts, HttpBody::from(body_bytes));
        Ok(RpcResponse::new(response, payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitized_target_omits_credentials() {
        let url: Uri = "https://provider.example:8545/v3/API_KEY?token=secret"
            .parse()
            .unwrap();
        assert_eq!(sanitized_target(&url), "provider.example:8545");

        let url: Uri = "https://user:password@provider.example:8545/v3/API_KEY"
            .parse()
            .unwrap();
        assert_eq!(sanitized_target(&url), "provider.example:8545");
    }
}
