use eyre::Result;
use jsonrpsee::{core::http_helpers, http_client::HttpBody};

#[derive(serde::Deserialize, Debug)]
struct Request<'a> {
    #[serde(borrow)]
    method: &'a str,
}

/// Decomposed JSON-RPC request.
#[derive(Clone, Debug)]
pub struct RpcRequest {
    pub parts: http::request::Parts,
    pub body: Vec<u8>,
    pub method: String,
}

impl RpcRequest {
    pub async fn from_request(request: http::Request<HttpBody>) -> Result<Self> {
        let (parts, body) = request.into_parts();
        let (body_bytes, _) = http_helpers::read_body(&parts.headers, body, u32::MAX).await?;

        let method = serde_json::from_slice::<Request>(&body_bytes)?
            .method
            .to_string();

        Ok(Self {
            parts,
            body: body_bytes,
            method,
        })
    }
}

impl Into<http::Request<HttpBody>> for RpcRequest {
    fn into(self) -> http::Request<HttpBody> {
        let body = HttpBody::from(self.body);
        let request = http::Request::from_parts(self.parts, body);
        request
    }
}

pub struct RpcResponse<T> {
    pub response: http::Response<T>,
    pub status: i32,
}

impl<T> RpcResponse<T> {
    pub fn new(response: http::Response<T>, status: i32) -> Self {
        Self { response, status }
    }

    // See: https://github.com/alloy-rs/alloy/blob/main/crates/rpc-types-eth/src/error.rs
    pub fn is_validation_error(&self) -> bool {
        self.status == -32000
            || self.status == -32003
            || self.status == 3
            || self.status == -32001
            || self.status == -39001
    }
}

pub fn parse_response_code(body_bytes: &[u8]) -> Result<Option<i32>> {
    #[derive(serde::Deserialize, Debug)]
    struct RpcResponse {
        error: Option<JsonRpcError>,
    }

    #[derive(serde::Deserialize, Debug)]
    struct JsonRpcError {
        code: i32,
    }

    let res = serde_json::from_slice::<RpcResponse>(body_bytes)?;

    Ok(res.error.map(|e| e.code))
}
