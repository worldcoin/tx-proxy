pub mod validation;

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use jsonrpsee::{
    core::BoxError,
    http_client::{HttpBody, HttpRequest, HttpResponse},
};
use tower::{Layer, Service};

use crate::{client::backend::Backend, utils::RpcRequest};

/// A [`Layer`] that validates responses from one backend prior to forwarding them to the next backend.
pub struct ProxyLayer {
    pub backend: Backend,
}

impl ProxyLayer {
    /// Creates a new [`ProxyLayer`] with the given backend.
    pub fn new(backend: Backend) -> Self {
        Self { backend }
    }
}

impl<S> Layer<S> for ProxyLayer {
    type Service = ProxyService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        ProxyService {
            backend: self.backend.clone(),
            inner,
        }
    }
}

#[derive(Clone)]
pub struct ProxyService<S> {
    backend: Backend,
    inner: S,
}

impl<S> Service<HttpRequest<HttpBody>> for ProxyService<S>
where
    S: Service<HttpRequest<HttpBody>, Response = HttpResponse> + Send + Sync + Clone + 'static,
    <S as Service<HttpRequest<HttpBody>>>::Response: 'static,
    <S as Service<HttpRequest<HttpBody>>>::Future: Send + 'static,
    <S as Service<HttpRequest<HttpBody>>>::Error: Into<BoxError> + Send,
{
    type Response = HttpResponse;
    type Error = BoxError;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, request: HttpRequest<HttpBody>) -> Self::Future {
        let mut service = self.clone();
        let mut backend = self.backend.clone();
        service.inner = std::mem::replace(&mut self.inner, service.inner);
        let fut = async move {
            let rpc_request = RpcRequest::from_request(request).await?;
            let result = backend.fan_request(rpc_request.clone()).await?;
            Ok::<HttpResponse<HttpBody>, BoxError>(result.0.response)
        };

        Box::pin(fut)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::{Arc, Mutex},
    };

    use super::{validation::ValidationLayer, *};
    use crate::client::http::HttpClient as TxProxyHttpClient;
    use alloy_primitives::{Bytes, bytes, hex};
    use alloy_rpc_types_engine::JwtSecret;
    use eyre::Result;
    use http::Uri;
    use http_body_util::BodyExt;
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;
    use jsonrpsee::{
        RpcModule,
        core::client::ClientT,
        http_client::HttpClient,
        server::{Server, ServerHandle},
        types::error::INTERNAL_ERROR_CODE,
    };
    use rollup_boost::HealthLayer;
    use serde_json::json;
    use tokio::{net::TcpListener, task::JoinHandle};

    struct TestHarness {
        builder_0: MockHttpServer,
        builder_1: MockHttpServer,
        builder_2: MockHttpServer,
        l2_0: MockHttpServer,
        l2_1: MockHttpServer,
        l2_2: MockHttpServer,
        server_handle: ServerHandle,
        proxy_client: HttpClient,
    }

    impl Drop for TestHarness {
        fn drop(&mut self) {
            self.server_handle.stop().unwrap();
        }
    }

    impl TestHarness {
        async fn new() -> eyre::Result<Self> {
            let builder_0 = MockHttpServer::serve().await?;
            let builder_1 = MockHttpServer::serve().await?;
            let builder_2 = MockHttpServer::serve().await?;
            let l2_0 = MockHttpServer::serve().await?;
            let l2_1 = MockHttpServer::serve().await?;
            let l2_2 = MockHttpServer::serve().await?;

            let builder_0_http_client = TxProxyHttpClient::new(
                format!("http://{}:{}", builder_0.addr.ip(), builder_0.addr.port())
                    .parse::<Uri>()?,
                JwtSecret::random(),
            );

            let builder_1_http_client = TxProxyHttpClient::new(
                format!("http://{}:{}", builder_1.addr.ip(), builder_1.addr.port())
                    .parse::<Uri>()?,
                JwtSecret::random(),
            );
            let builder_2_http_client = TxProxyHttpClient::new(
                format!("http://{}:{}", builder_2.addr.ip(), builder_2.addr.port())
                    .parse::<Uri>()?,
                JwtSecret::random(),
            );

            let l2_0_http_client = TxProxyHttpClient::new(
                format!("http://{}:{}", l2_0.addr.ip(), l2_0.addr.port()).parse::<Uri>()?,
                JwtSecret::random(),
            );

            let l2_1_http_client = TxProxyHttpClient::new(
                format!("http://{}:{}", l2_1.addr.ip(), l2_1.addr.port()).parse::<Uri>()?,
                JwtSecret::random(),
            );

            let l2_2_http_client = TxProxyHttpClient::new(
                format!("http://{}:{}", l2_2.addr.ip(), l2_2.addr.port()).parse::<Uri>()?,
                JwtSecret::random(),
            );

            let builder_backend = Backend {
                client_0: builder_0_http_client,
                client_1: builder_1_http_client,
                client_2: builder_2_http_client,
            };

            let l2_backend = Backend {
                client_0: l2_0_http_client,
                client_1: l2_1_http_client,
                client_2: l2_2_http_client,
            };
            let middleware = tower::ServiceBuilder::new()
                .layer(HealthLayer)
                .layer(ValidationLayer::new(builder_backend))
                .layer(ProxyLayer::new(l2_backend));
            let temp_listener = TcpListener::bind("0.0.0.0:0").await?;
            let server_addr = temp_listener.local_addr()?;

            drop(temp_listener);

            let server = Server::builder()
                .set_http_middleware(middleware)
                .build(server_addr)
                .await?;

            let server_addr = server.local_addr()?;
            let proxy_client: HttpClient = HttpClient::builder().build(format!(
                "http://{}:{}",
                server_addr.ip(),
                server_addr.port()
            ))?;

            let server_handle = server.start(RpcModule::new(()));

            Ok(Self {
                builder_0,
                builder_1,
                builder_2,
                l2_0,
                l2_1,
                l2_2,
                server_handle,
                proxy_client,
            })
        }
    }
    struct MockHttpServer {
        addr: SocketAddr,
        requests: Arc<Mutex<Vec<serde_json::Value>>>,
        join_handle: JoinHandle<()>,
    }

    impl Drop for MockHttpServer {
        fn drop(&mut self) {
            self.join_handle.abort();
        }
    }

    impl MockHttpServer {
        async fn serve() -> eyre::Result<Self> {
            let listener = TcpListener::bind("0.0.0.0:0").await?;
            let addr = listener.local_addr()?;
            let requests = Arc::new(Mutex::new(vec![]));

            let requests_clone = requests.clone();
            let handle = tokio::spawn(async move {
                loop {
                    match listener.accept().await {
                        Ok((stream, _)) => {
                            let io = TokioIo::new(stream);
                            let requests = requests_clone.clone();

                            tokio::spawn(async move {
                                if let Err(err) = hyper::server::conn::http1::Builder::new()
                                    .serve_connection(
                                        io,
                                        service_fn(move |req| {
                                            Self::handle_request(req, requests.clone())
                                        }),
                                    )
                                    .await
                                {
                                    eprintln!("Error serving connection: {}", err);
                                }
                            });
                        }
                        Err(e) => eprintln!("Error accepting connection: {}", e),
                    }
                }
            });

            Ok(Self {
                addr,
                requests,
                join_handle: handle,
            })
        }

        async fn handle_request(
            req: hyper::Request<hyper::body::Incoming>,
            requests: Arc<Mutex<Vec<serde_json::Value>>>,
        ) -> Result<hyper::Response<String>, hyper::Error> {
            let body_bytes = match req.into_body().collect().await {
                Ok(buf) => buf.to_bytes(),
                Err(_) => {
                    let error_response = json!({
                        "jsonrpc": "2.0",
                        "error": { "code": -32700, "message": "Failed to read request body" },
                        "id": null
                    });
                    return Ok(hyper::Response::new(error_response.to_string()));
                }
            };

            let request_body: serde_json::Value = match serde_json::from_slice(&body_bytes) {
                Ok(json) => json,
                Err(_) => {
                    let error_response = json!({
                        "jsonrpc": "2.0",
                        "error": { "code": -32700, "message": "Invalid JSON format" },
                        "id": null
                    });
                    return Ok(hyper::Response::new(error_response.to_string()));
                }
            };

            requests.lock().unwrap().push(request_body.clone());

            let method = request_body["method"].as_str().unwrap_or_default();

            let response = match method {
                "eth_sendRawTransactionPass" => json!({
                    "jsonrpc": "2.0",
                    "result": format!("{}", bytes!("1234")),
                    "id": request_body["id"]
                }),
                "eth_sendRawTransactionValidationFail" => {
                    let error_response = json!({
                        "jsonrpc": "2.0",
                        "error": { "code": INTERNAL_ERROR_CODE, "message": "PBH Transaction Validation Failed" },
                        "id": request_body["id"]
                    });
                    return Ok(hyper::Response::new(error_response.to_string()));
                }
                _ => {
                    let error_response = json!({
                        "jsonrpc": "2.0",
                        "error": { "code": -32601, "message": "Method not found" },
                        "id": request_body["id"]
                    });
                    return Ok(hyper::Response::new(error_response.to_string()));
                }
            };

            Ok(hyper::Response::new(response.to_string()))
        }
    }

    #[cfg(test)]
    #[ctor::ctor]
    fn crypto_ring_init() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .unwrap();
    }

    #[tokio::test]
    async fn test_send_raw_transaction_happy_path() -> eyre::Result<()> {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let test_harness = TestHarness::new().await?;

        let expected_tx: Bytes = hex!("1234").into();
        let expected_method = "eth_sendRawTransactionPass";

        test_harness
            .proxy_client
            .request::<serde_json::Value, _>(expected_method, (expected_tx.clone(),))
            .await?;

        let expected_tx = json!(expected_tx);

        // Assert the builders received the correct payload
        let builder_0 = &test_harness.builder_0;
        let builder_requests = builder_0.requests.lock().unwrap();
        let builder_req = builder_requests.first().unwrap();
        assert_eq!(builder_requests.len(), 1);
        assert_eq!(builder_req["method"], expected_method);
        assert_eq!(builder_req["params"][0], expected_tx);

        let builder_1 = &test_harness.builder_1;
        let builder_requests = builder_1.requests.lock().unwrap();
        let builder_req = builder_requests.first().unwrap();
        assert_eq!(builder_requests.len(), 1);
        assert_eq!(builder_req["method"], expected_method);
        assert_eq!(builder_req["params"][0], expected_tx);

        let builder_2 = &test_harness.builder_2;
        let builder_requests = builder_2.requests.lock().unwrap();
        let builder_req = builder_requests.first().unwrap();
        assert_eq!(builder_requests.len(), 1);
        assert_eq!(builder_req["method"], expected_method);
        assert_eq!(builder_req["params"][0], expected_tx);

        // Because the request to the l2 backend is non blocking on the future returned from the validation service
        // We need to sleep the thread here
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // assert the l2s also received the correct payload
        let l2_0 = &test_harness.l2_0;
        let l2_requests = l2_0.requests.lock().unwrap();
        let l2_req = l2_requests.first().unwrap();
        assert_eq!(l2_requests.len(), 1);
        assert_eq!(l2_req["method"], expected_method);
        assert_eq!(l2_req["params"][0], expected_tx);

        let l2_1 = &test_harness.l2_1;
        let l2_requests = l2_1.requests.lock().unwrap();
        let l2_req = l2_requests.first().unwrap();
        assert_eq!(l2_requests.len(), 1);
        assert_eq!(l2_req["method"], expected_method);
        assert_eq!(l2_req["params"][0], expected_tx);

        let l2_2 = &test_harness.l2_2;
        let l2_requests = l2_2.requests.lock().unwrap();
        let l2_req = l2_requests.first().unwrap();
        assert_eq!(l2_requests.len(), 1);
        assert_eq!(l2_req["method"], expected_method);
        assert_eq!(l2_req["params"][0], expected_tx);
        Ok(())
    }

    #[tokio::test]
    async fn test_send_raw_transaction_sad_path() -> Result<()> {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let test_harness = TestHarness::new().await?;

        let send_request = async |method| {
            let _ = test_harness
                .proxy_client
                .request::<serde_json::Value, [String; 0]>(method, [])
                .await;
        };

        let assert_validation_fail_case = async |test_harness: &TestHarness, expected_length| {
            // Assert the builders received the correct payload
            let builder_0 = &test_harness.builder_0;
            let builder_requests = builder_0.requests.lock().unwrap();
            assert_eq!(builder_requests.len(), expected_length);

            let builder_1 = &test_harness.builder_1;
            let builder_requests = builder_1.requests.lock().unwrap();
            assert_eq!(builder_requests.len(), expected_length);

            let builder_2 = &test_harness.builder_2;
            let builder_requests = builder_2.requests.lock().unwrap();
            assert_eq!(builder_requests.len(), expected_length);

            // Because the request to the l2 backend is non blocking on the future returned from the validation service
            // We need to sleep the thread here
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

            // assert the l2s didn't received the payload
            let l2_0 = &test_harness.l2_0;
            let l2_requests = l2_0.requests.lock().unwrap();
            assert_eq!(l2_requests.len(), 0);

            let l2_1 = &test_harness.l2_1;
            let l2_requests = l2_1.requests.lock().unwrap();
            assert_eq!(l2_requests.len(), 0);

            let l2_2 = &test_harness.l2_2;
            let l2_requests = l2_2.requests.lock().unwrap();
            assert_eq!(l2_requests.len(), 0);
        };

        send_request("eth_sendRawTransactionValidationFail").await;
        assert_validation_fail_case(&test_harness, 1).await;

        Ok(())
    }
}
