use crate::client::HttpClient;
use crate::rpc::{RpcRequest, RpcResponse};
use eyre::eyre;
use futures::future::join_all;
use jsonrpsee::{core::BoxError, http_client::HttpBody};
use tracing::{error, warn};

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
        let mut responses = Vec::with_capacity(results.len());
        let mut failures = Vec::new();

        for (client, result) in self.targets.iter().zip(results) {
            match result {
                Ok(response) => responses.push(response),
                Err(error) => failures.push((client.log_target(), error.to_string())),
            }
        }

        if responses.is_empty() {
            error!(failures = ?failures, "All requests failed");
            return Err(eyre!("All requests failed. No valid responses received.").into());
        }

        if !failures.is_empty() {
            warn!(
                successful_requests = responses.len(),
                failed_requests = failures.len(),
                failures = ?failures,
                "Some requests failed"
            );
        }

        Ok(responses)
    }
}
