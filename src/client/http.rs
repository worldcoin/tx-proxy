use crate::types::{RpcRequest, RpcResponse};
use alloy_rpc_types_engine::JwtSecret;
use http::Uri;
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use jsonrpsee::{core::BoxError, http_client::HttpBody};
use tower::ServiceBuilder;
use tower_http::decompression::{Decompression, DecompressionLayer};

use super::auth::{AuthClientLayer, AuthService};

pub type HttpClientService =
    Decompression<AuthService<Client<HttpsConnector<HttpConnector>, HttpBody>>>;

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

    pub async fn forward<T>(&self, req: RpcRequest) -> Result<RpcResponse<T>, BoxError> {
        todo!()
    }
}
