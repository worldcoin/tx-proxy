use crate::fanout::FanoutWrite;
use crate::rpc::RpcRequest;
use jsonrpsee::{
    core::BoxError,
    http_client::{HttpBody, HttpRequest, HttpResponse},
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tower::{Layer, Service};
use tracing::instrument;

/// A [`Layer`] that validates responses from one fanout prior to forwarding them to the next fanout.
pub struct ProxyLayer {
    pub fanout: FanoutWrite,
}

impl ProxyLayer {
    /// Creates a new [`ProxyLayer`] with the given fanout.
    pub fn new(fanout: FanoutWrite) -> Self {
        Self { fanout }
    }
}

impl<S> Layer<S> for ProxyLayer {
    type Service = ProxyService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        ProxyService {
            fanout: self.fanout.clone(),
            inner,
        }
    }
}

#[derive(Clone)]
pub struct ProxyService<S> {
    fanout: FanoutWrite,
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

    #[instrument(skip(self, request), target = "tx-proxy::proxy")]
    fn call(&mut self, request: HttpRequest<HttpBody>) -> Self::Future {
        let mut service = self.clone();
        let mut fanout = self.fanout.clone();
        service.inner = std::mem::replace(&mut self.inner, service.inner);
        let fut = async move {
            let rpc_request = RpcRequest::from_request(request).await?;
            let mut result = fanout.fan_request(rpc_request.clone()).await?;

            Ok::<HttpResponse<HttpBody>, BoxError>(result.remove(0).response)
        };

        Box::pin(fut)
    }
}
