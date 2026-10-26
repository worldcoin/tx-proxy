use crate::client::HttpClient;
use crate::metrics::ProxyMetrics;
use crate::rpc::{RpcRequest, RpcResponse};
use eyre::eyre;
use futures::future::join_all;
use jsonrpsee::{core::BoxError, http_client::HttpBody};
use tracing::{error, warn};

#[derive(Clone, Copy, Debug)]
pub enum FanoutKind {
    Builder,
    L2,
}

impl FanoutKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Builder => "builder",
            Self::L2 => "l2",
        }
    }
}

/// A FanoutWrite for fanning JSON-RPC requests to multiple
/// Clients in a High Availability configuration.
#[derive(Clone, Debug)]
pub struct FanoutWrite {
    pub targets: Vec<HttpClient>,
    kind: FanoutKind,
}

impl FanoutWrite {
    /// Creates a new [`FanoutWrite`] with the given clients.
    pub fn new(targets: Vec<HttpClient>, kind: FanoutKind) -> Self {
        Self { targets, kind }
    }

    /// Sends a JSON-RPC request to all clients and return the responses.
    pub async fn fan_request(
        &mut self,
        req: RpcRequest,
        metrics: &ProxyMetrics,
    ) -> Result<Vec<RpcResponse<HttpBody>>, BoxError> {
        let fut = self
            .targets
            .iter_mut()
            .map(|client| client.forward(req.clone()))
            .collect::<Vec<_>>();

        let results = join_all(fut).await;
        let mut responses = Vec::with_capacity(results.len());
        let mut failures = Vec::new();

        for (client, result) in self.targets.iter().zip(results) {
            let target = client.log_target();
            metrics.record_fanout_target_health(self.kind, &target, result.is_ok());

            match result {
                Ok(response) => responses.push(response),
                Err(error) => failures.push((target, error.to_string())),
            }
        }

        if responses.is_empty() {
            metrics.record_fanout_total_failure(self.kind);
            error!(failures = ?failures, "All requests failed");
            return Err(eyre!("All requests failed. No valid responses received.").into());
        }

        if !failures.is_empty() {
            warn!(
                successful_requests = responses.len(),
                failed_requests = failures.len(),
                failures = ?failures,
                "Some requests failed"
            );
        }

        Ok(responses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::with_local_recorder;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};

    #[test]
    fn total_failure_is_recorded_before_returning_an_error() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let request = RpcRequest {
            parts: http::Request::new(()).into_parts().0,
            body: Vec::new(),
            method: "eth_sendRawTransaction".to_owned(),
        };

        let result = with_local_recorder(&recorder, || {
            let mut fanout = FanoutWrite::new(Vec::new(), FanoutKind::L2);
            let metrics = ProxyMetrics::default();
            futures::executor::block_on(fanout.fan_request(request, &metrics))
        });
        assert!(result.is_err());

        let metrics = snapshotter.snapshot().into_vec();
        assert!(metrics.iter().any(|(key, _, _, value)| {
            let key = key.key();
            let key_name = key.name();
            let is_l2_fanout = key
                .labels()
                .any(|label| label.key() == "fanout" && label.value() == "l2");

            key_name == "fanout_total_failures" && is_l2_fanout && *value == DebugValue::Counter(1)
        }));
    }

    #[test]
    fn target_health_is_labeled_with_safe_target_and_kind() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();

        with_local_recorder(&recorder, || {
            let metrics = ProxyMetrics::default();
            metrics.record_fanout_target_health(
                FanoutKind::Builder,
                "builder.internal:8545",
                false,
            );
        });

        let metrics = snapshotter.snapshot().into_vec();
        assert!(metrics.iter().any(|(key, _, _, value)| {
            let key = key.key();
            let key_name = key.name();
            let is_builder_fanout = key
                .labels()
                .any(|label| label.key() == "fanout" && label.value() == "builder");
            let is_correct_target = key
                .labels()
                .any(|label| label.key() == "target" && label.value() == "builder.internal:8545");

            key_name == "fanout_target_healthy"
                && is_builder_fanout
                && is_correct_target
                && *value == DebugValue::Gauge(0.0.into())
        }));
    }
}
