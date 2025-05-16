use crate::client::HttpClient;
use crate::rpc::{RpcRequest, RpcResponse};
use eyre::eyre;
use futures::future::join_all;
use jsonrpsee::{core::BoxError, http_client::HttpBody};
use tracing::error;

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

        let results = join_all(fut).await;
        let responses = results
            .into_iter()
            .filter_map(|res| match res {
                Ok(resp) => Some(resp),
                Err(err) => {
                    error!(%err, "Request failed");
                    None
                }
            })
            .collect::<Vec<_>>();

        if responses.is_empty() {
            return Err(eyre!("All requests failed. No valid responses received.").into());
        }

        Ok(responses)
    }
}
