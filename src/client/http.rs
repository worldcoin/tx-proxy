use crate::utils::{RpcRequest, RpcResponse, parse_response_code};
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
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_http::decompression::{Decompression, DecompressionLayer};
use tracing::{debug, error, instrument};

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

        let code = if let Some(code) = parse_response_code(&body_bytes)? {
            error!(%code, "error in forwarded response");
            code
        } else {
            0
        };

        let response = http::Response::from_parts(parts, HttpBody::from(body_bytes));
        Ok(RpcResponse::new(response, code))
    }
}
