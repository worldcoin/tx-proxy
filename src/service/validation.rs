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
pub struct ValidationLayer {
    pub backend: Backend,
}

impl ValidationLayer {
    /// Creates a new [`ValidationLayer`] with the given backend.
    pub fn new(backend: Backend) -> Self {
        Self { backend }
    }
}

impl<S> Layer<S> for ValidationLayer {
    type Service = ValidationService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        ValidationService {
            backend: self.backend.clone(),
            inner,
        }
    }
}

#[derive(Clone)]
pub struct ValidationService<S> {
    backend: Backend,
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

    fn call(&mut self, request: HttpRequest<HttpBody>) -> Self::Future {
        let mut service = self.clone();
        let mut backend = self.backend.clone();
        service.inner = std::mem::replace(&mut self.inner, service.inner);

        let fut = async move {
            let rpc_request = RpcRequest::from_request(request).await?;
            let result = backend.fan_request(rpc_request.clone()).await?;
            let (res_0, _, _) = result; // TODO: Think about how we want to handle the other responses

            if !res_0.is_validation_error() {
                tokio::spawn(async move { service.inner.call(rpc_request.into()).await });
            }

            Ok::<HttpResponse<HttpBody>, BoxError>(res_0.response)
        };

        Box::pin(fut)
    }
}
