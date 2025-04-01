use jsonrpsee::http_client::HttpBody;

/// Decomposed JSON-RPC request.
#[derive(Clone, Debug)]
pub struct RpcRequest {
    pub parts: http::request::Parts,
    pub body: Vec<u8>,
    pub method: String,
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

    pub fn is_validation_error(&self) -> bool {
        // TODO: Isolate validation error codes
        self.status != 0
    }
}
