use crate::client::HttpClient;
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
