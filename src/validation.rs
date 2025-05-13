use std::{
    pin::Pin,
    task::{Context, Poll},
};

use eyre::eyre;
use jsonrpsee::{
    core::BoxError,
    http_client::{HttpBody, HttpRequest, HttpResponse},
};
use tower::{Layer, Service};
use tracing::{debug, instrument};

use crate::{fanout::FanoutWrite, rpc::RpcRequest};

pub const ALLOWED_METHODS: &[&str; 3] = &[
    "eth_sendRawTransactionPass",
    "eth_sendRawTransactionConditional",
    "eth_chainId",
];

fn check_allowed_methods(method: &str) -> eyre::Result<()> {
    if ALLOWED_METHODS.contains(&method) {
        Ok(())
    } else {
        Err(eyre!("Disallowed method: {}", method))
    }
}

/// A [`Layer`] that validates responses from one fanout prior to forwarding them to the next fanout.
pub struct ValidationLayer {
    pub fanout: FanoutWrite,
}

impl ValidationLayer {
    /// Creates a new [`ValidationLayer`] with the given fanout.
    pub fn new(fanout: FanoutWrite) -> Self {
        Self { fanout }
    }
}

impl<S> Layer<S> for ValidationLayer {
    type Service = ValidationService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        ValidationService {
            fanout: self.fanout.clone(),
            inner,
        }
    }
}

#[derive(Clone)]
pub struct ValidationService<S> {
    fanout: FanoutWrite,
    inner: S,
}

impl<S> Service<HttpRequest<HttpBody>> for ValidationService<S>
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

    #[instrument(skip(self, request), target = "tx-proxy::validation")]
    fn call(&mut self, request: HttpRequest<HttpBody>) -> Self::Future {
        let mut service = self.clone();
        let mut fanout = self.fanout.clone();
        service.inner = std::mem::replace(&mut self.inner, service.inner);

        let fut = async move {
            let rpc_request = RpcRequest::from_request(request).await?;
            check_allowed_methods(&rpc_request.method)?;

            debug!(target: "tx-proxy::validation", method = %rpc_request.method, "forwarding request to builder fanout");

            let mut responses = fanout.fan_request(rpc_request.clone()).await?;
            if responses.iter().all(|res| !res.pbh_error()) {
                debug!(target: "tx-proxy::validation", method = %rpc_request.method, "forwarding request to l2 fanout");
                tokio::spawn(async move {
                    let _ = service.inner.call(rpc_request.into()).await;
                });
            }

            let res_0 = responses.remove(0).response;

            // Loop through each response, if pbh error, break
            // otherwise if the response is valid, set the response
            let mut response = None;
            for res in responses {
                // If the response is a pbh error, short circuit
                if res.pbh_error() {
                    response = Some(res.response);
                    break;
                }
                // If the response has not been set and res is not err, set the response
                if response.is_none() && !res.is_error() {
                    response = Some(res.response);
                }
            }

            Ok::<HttpResponse<HttpBody>, BoxError>(response.unwrap_or(res_0))
        };

        Box::pin(fut)
    }
}
