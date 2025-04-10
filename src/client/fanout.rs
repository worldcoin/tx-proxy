use jsonrpsee::{core::BoxError, http_client::HttpBody};
use tokio::join;

use crate::utils::{RpcRequest, RpcResponse};

use super::http::HttpClient;

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
        let (res_0, res_1, res_2) = join!(
            self.client_0.forward(req.clone()),
            self.client_1.forward(req.clone()),
            self.client_2.forward(req)
        );

        Ok((res_0?, res_1?, res_2?))
    }
}
