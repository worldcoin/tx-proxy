use eyre::Result;
use jsonrpsee::{
    core::http_helpers,
    http_client::HttpBody,
    types::{ErrorObjectOwned, Request, Response, ResponsePayload, error::INTERNAL_ERROR_CODE},
};
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

impl From<RpcRequest> for http::Request<HttpBody> {
    fn from(val: RpcRequest) -> http::Request<HttpBody> {
        let body = HttpBody::from(val.body);
        http::Request::from_parts(val.parts, body)
    }
}

pub struct RpcResponse<T> {
    pub response: http::Response<T>,
    pub error: Option<ErrorObjectOwned>,
}

impl<T> RpcResponse<T> {
    pub fn new(response: http::Response<T>, error: Option<ErrorObjectOwned>) -> Self {
        Self { response, error }
    }

    pub fn pbh_error(&self) -> bool {
        if let Some(ref error) = self.error {
            return error.code() == INTERNAL_ERROR_CODE
                && error
                    .message()
                    .starts_with("PBH Transaction Validation Failed");
        }
        false
    }
}

pub fn parse_response_payload(body_bytes: &[u8]) -> Result<Option<ErrorObjectOwned>> {
    let res = serde_json::from_slice::<Response<serde_json::Value>>(body_bytes)?;
    let payload = res.payload;
    match payload {
        ResponsePayload::Error(obj) => Ok(Some(obj.into_owned())),
        _ => return Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Response;
    use jsonrpsee::core::BoxError;

    #[tokio::test]
    async fn test_parse_error_response_payload() -> Result<(), BoxError> {
        let http_response = http::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .body(HttpBody::from(r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"PBH Transaction Validation Failed: Invalid calldata encoding"},"id":1}"#))
            .unwrap();
        let (parts, body) = http_response.into_parts();
        let body_bytes = http_helpers::read_body(&parts.headers, body, u32::MAX)
            .await?
            .0;

        let payload = RpcResponse::new(
            Response::from_parts(parts, HttpBody::from(body_bytes.clone())),
            parse_response_payload(&body_bytes).expect("Failed to parse payload"),
        );
        assert!(payload.pbh_error());

        Ok(())
    }

    #[tokio::test]
    async fn test_parse_success_response_payload() -> Result<(), BoxError> {
        let http_response = http::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .body(HttpBody::from(r#"{"jsonrpc":"2.0","result":"ok","id":1}"#))
            .unwrap();
        let (parts, body) = http_response.into_parts();
        let body_bytes = http_helpers::read_body(&parts.headers, body, u32::MAX)
            .await?
            .0;

        let payload = RpcResponse::new(
            Response::from_parts(parts, HttpBody::from(body_bytes.clone())),
            parse_response_payload(&body_bytes).expect("Failed to parse payload"),
        );
        assert!(!payload.pbh_error());

        Ok(())
    }
}
