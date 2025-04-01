use jsonrpsee::core::BoxError;
use tokio::join;

use crate::types::{RpcRequest, RpcResponse};

use super::http::HttpClient;

/// A Backend for fanning JSON-RPC requests to multiple
/// Clients in a High Availability configuration.
#[derive(Clone, Debug)]
pub struct Backend {
    pub(crate) client_0: HttpClient,
    pub(crate) client_1: HttpClient,
    pub(crate) client_2: HttpClient,
}

impl Backend {
    /// Creates a new [`Backend`] with the given clients.
    pub fn new(client_0: HttpClient, client_1: HttpClient, client_2: HttpClient) -> Self {
        Self {
            client_0,
            client_1,
            client_2,
        }
    }

    /// Sends a JSON-RPC request to all clients and return the responses.
    pub async fn fan<T>(
        &self,
        request: RpcRequest,
    ) -> Result<(RpcResponse<T>, RpcResponse<T>, RpcResponse<T>), BoxError> {
        let (res_0, res_1, res_2) = join!(
            self.client_0.forward(request.clone()),
            self.client_1.forward(request.clone()),
            self.client_2.forward(request)
        );

        Ok((res_0?, res_1?, res_2?))
    }
}
