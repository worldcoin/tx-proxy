use crate::utils::{RpcRequest, RpcResponse, parse_response_payload};
use alloy_rpc_types_engine::JwtSecret;
use futures::future::try_join_all;
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
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_http::decompression::{Decompression, DecompressionLayer};
use tracing::{debug, instrument};

pub type HttpClientService =
    Decompression<AuthClientService<Client<HttpsConnector<HttpConnector>, HttpBody>>>;

#[derive(Clone, Debug)]
pub(crate) struct HttpClient {
    client: HttpClientService,
    url: Uri,
}

impl HttpClient {
    pub fn new(url: Uri, secret: JwtSecret) -> Self {
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .expect("no native root CA certificates found")
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();

        let client = Client::builder(TokioExecutor::new()).build(connector);

        let client = ServiceBuilder::new()
            .layer(DecompressionLayer::new())
            .layer(AuthClientLayer::new(secret))
            .service(client);

        Self { client, url }
    }

    #[instrument(
        skip(self, req),
        target = "tx-proxy::http::forward",
        fields(otel.kind = ?SpanKind::Client),
        err(Debug)
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
/// A FanoutWrite for fanning JSON-RPC requests to multiple
/// Clients in a High Availability configuration.
#[derive(Clone, Debug)]
pub struct FanoutWrite {
    pub targets: Vec<HttpClient>,
}

impl FanoutWrite {
    /// Creates a new [`FanoutWrite`] with the given clients.
    pub fn new(targets: Vec<HttpClient>) -> Self {
        Self { targets }
    }

    /// Sends a JSON-RPC request to all clients and return the responses.
    pub async fn fan_request(
        &mut self,
        req: RpcRequest,
    ) -> Result<Vec<RpcResponse<HttpBody>>, BoxError> {
        let fut = self
            .targets
            .iter_mut()
            .map(|client| client.forward(req.clone()))
            .collect::<Vec<_>>();

        let responses = try_join_all(fut).await?;
        Ok(responses)
    }
}
