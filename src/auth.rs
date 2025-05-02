use pin_project::pin_project;
use std::{
    pin::Pin,
    task::{Context, Poll},
};

use alloy_rpc_types_engine::{JwtError, JwtSecret};
use http::{HeaderMap, Response, StatusCode, header};
use jsonrpsee::{
    http_client::{HttpBody, HttpResponse},
    server::HttpRequest,
};
use tower::{Layer, Service};
use tracing::error;

pub struct AuthLayer {
    validator: JwtAuthValidator,
}

impl AuthLayer {
    /// Creates an instance of [`AuthLayer`].
    pub const fn new(validator: JwtAuthValidator) -> Self {
        Self { validator }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            validator: self.validator.clone(),
            inner,
        }
    }
}

/// This type is the actual implementation of the middleware. It follows the [`Service`]
/// specification to correctly proxy Http requests to its inner service after headers validation.
#[derive(Clone, Debug)]
pub struct AuthService<S> {
    /// Performs auth validation logics
    validator: JwtAuthValidator,
    /// Recipient of authorized Http requests
    inner: S,
}

impl<S> Service<HttpRequest> for AuthService<S>
where
    S: Service<HttpRequest, Response = HttpResponse>,
    Self: Clone,
{
    type Response = HttpResponse;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    /// If we get polled it means that we dispatched an authorized Http request to the inner layer.
    /// So we just poll the inner layer ourselves.
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    /// This is the entrypoint of the service. We receive an Http request and check the validity of
    /// the authorization header.
    ///
    /// Returns a future that wraps either:
    /// - The inner service future for authorized requests
    /// - An error Http response in case of authorization errors
    fn call(&mut self, req: HttpRequest) -> Self::Future {
        match self.validator.validate(req.headers()) {
            Ok(_) => ResponseFuture::future(self.inner.call(req)),
            Err(res) => ResponseFuture::invalid_auth(res),
        }
    }
}

/// A future representing the response of an RPC request
#[pin_project]
pub struct ResponseFuture<F> {
    /// The kind of response future, error or pending
    #[pin]
    kind: Kind<F>,
}

impl<F> ResponseFuture<F> {
    const fn future(future: F) -> Self {
        Self {
            kind: Kind::Future { future },
        }
    }

    const fn invalid_auth(err_res: HttpResponse) -> Self {
        Self {
            kind: Kind::Error {
                response: Some(err_res),
            },
        }
    }
}

#[pin_project(project = KindProj)]
enum Kind<F> {
    Future {
        #[pin]
        future: F,
    },
    Error {
        response: Option<HttpResponse>,
    },
}

impl<F, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<HttpResponse, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().kind.project() {
            KindProj::Future { future } => future.poll(cx),
            KindProj::Error { response } => {
                let response = response.take().unwrap();
                Poll::Ready(Ok(response))
            }
        }
    }
}

/// Implements JWT validation logics and integrates
/// to an Http [`AuthLayer`][crate::AuthLayer]
/// by implementing the [`AuthValidator`] trait.
#[derive(Debug, Clone)]
pub struct JwtAuthValidator {
    secret: JwtSecret,
}

impl JwtAuthValidator {
    /// Creates a new instance of [`JwtAuthValidator`].
    /// Validation logics are implemented by the `secret`
    /// argument (see [`JwtSecret`]).
    pub const fn new(secret: JwtSecret) -> Self {
        Self { secret }
    }
}

impl JwtAuthValidator {
    pub fn validate(&self, headers: &HeaderMap) -> Result<(), HttpResponse> {
        match get_bearer(headers) {
            Some(jwt) => match self.secret.validate(&jwt) {
                Ok(_) => Ok(()),
                Err(e) => {
                    error!(target: "tx-proxy::jwt-validator", "Invalid JWT: {e}");
                    let response = err_response(e);
                    Err(response)
                }
            },
            None => {
                let e = JwtError::MissingOrInvalidAuthorizationHeader;
                error!(target: "tx-proxy::jwt-validator", "Invalid JWT: {e}");
                let response = err_response(e);
                Err(response)
            }
        }
    }
}

/// This is an utility function that retrieves a bearer
/// token from an authorization Http header.
fn get_bearer(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(header::AUTHORIZATION)?;
    let auth: &str = header.to_str().ok()?;
    let prefix = "Bearer ";
    let index = auth.find(prefix)?;
    let token: &str = &auth[index + prefix.len()..];
    Some(token.into())
}

fn err_response(err: JwtError) -> HttpResponse {
    // We build a response from an error message.
    // We don't cope with headers or other structured fields.
    // Then we are safe to "expect" on the result.
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .body(HttpBody::new(err.to_string()))
        .expect("This should never happen")
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_rpc_types_engine::{Claims, JwtError, JwtSecret};
    use jsonrpsee::{
        RpcModule,
        server::{RandomStringIdProvider, ServerBuilder, ServerHandle},
    };
    use reqwest::{StatusCode, header};
    use std::{
        net::SocketAddr,
        time::{SystemTime, UNIX_EPOCH},
    };

    const AUTH_PORT: u32 = 8551;
    const AUTH_ADDR: &str = "0.0.0.0";
    const SECRET: &str = "f79ae8046bc11c9927afe911db7143c51a806c4a537cc08e0d37140b0192f430";

    #[tokio::test]
    async fn test_jwt_layer() {
        // We group all tests into one to avoid individual #[tokio::test]
        // to concurrently spawn a server on the same port.
        valid_jwt().await;
        missing_jwt_error().await;
        wrong_jwt_signature_error().await;
        invalid_issuance_timestamp_error().await;
        jwt_decode_error().await
    }

    async fn valid_jwt() {
        let claims = Claims {
            iat: to_u64(SystemTime::now()),
            exp: Some(10000000000),
        };
        let secret = JwtSecret::from_hex(SECRET).unwrap(); // Same secret as the server
        let jwt = secret.encode(&claims).unwrap();
        let (status, _) = send_request(Some(jwt)).await;
        assert_eq!(status, StatusCode::OK);
    }

    async fn missing_jwt_error() {
        let (status, body) = send_request(None).await;
        let expected = JwtError::MissingOrInvalidAuthorizationHeader;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, expected.to_string());
    }

    async fn wrong_jwt_signature_error() {
        // This secret is different from the server. This will generate a
        // different signature
        let secret = JwtSecret::random();
        let claims = Claims {
            iat: to_u64(SystemTime::now()),
            exp: Some(10000000000),
        };
        let jwt = secret.encode(&claims).unwrap();

        let (status, body) = send_request(Some(jwt)).await;
        let expected = JwtError::InvalidSignature;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, expected.to_string());
    }

    async fn invalid_issuance_timestamp_error() {
        let secret = JwtSecret::from_hex(SECRET).unwrap(); // Same secret as the server

        let iat = to_u64(SystemTime::now()) + 1000;
        let claims = Claims {
            iat,
            exp: Some(10000000000),
        };
        let jwt = secret.encode(&claims).unwrap();

        let (status, body) = send_request(Some(jwt)).await;
        let expected = JwtError::InvalidIssuanceTimestamp;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, expected.to_string());
    }

    async fn jwt_decode_error() {
        let jwt = "this jwt has serious encoding problems".to_string();
        let (status, body) = send_request(Some(jwt)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, "JWT decoding error: InvalidToken".to_string());
    }

    async fn send_request(jwt: Option<String>) -> (StatusCode, String) {
        let server = spawn_server().await;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(1))
            .build()
            .unwrap();

        let body = r#"{"jsonrpc": "2.0", "method": "greet_melkor", "params": [], "id": 1}"#;
        let response = client
            .post(format!("http://{AUTH_ADDR}:{AUTH_PORT}"))
            .bearer_auth(jwt.unwrap_or_default())
            .body(body)
            .header(header::CONTENT_TYPE, "application/json")
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();

        server.stop().unwrap();
        server.stopped().await;

        (status, body)
    }

    /// Spawn a new RPC server equipped with a `JwtLayer` auth middleware.
    async fn spawn_server() -> ServerHandle {
        let secret = JwtSecret::from_hex(SECRET).unwrap();
        let addr = format!("{AUTH_ADDR}:{AUTH_PORT}");
        let validator = JwtAuthValidator::new(secret);
        let layer = AuthLayer::new(validator);
        let middleware = tower::ServiceBuilder::default().layer(layer);

        // Create a layered server
        let server = ServerBuilder::default()
            .set_id_provider(RandomStringIdProvider::new(16))
            .set_http_middleware(middleware)
            .build(addr.parse::<SocketAddr>().unwrap())
            .await
            .unwrap();

        // Create a mock rpc module
        let mut module = RpcModule::new(());
        module
            .register_method("greet_melkor", |_, _, _| "You are the dark lord")
            .unwrap();

        server.start(module)
    }

    fn to_u64(time: SystemTime) -> u64 {
        time.duration_since(UNIX_EPOCH).unwrap().as_secs()
    }
}
