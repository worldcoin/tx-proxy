use crate::fanout::FanoutKind;
use metrics::{Counter, Histogram, counter, describe_counter, describe_gauge, gauge};
use metrics_derive::Metrics;

#[derive(Metrics)]
#[metrics(scope = "metrics")]
pub struct ProxyMetrics {
    /// L2 Requests Latency
    #[metric(describe = "L2 Requests Latency in seconds")]
    pub l2_requests_latency: Histogram,
    /// Builder Requests Latency
    #[metric(describe = "Builder Requests Latency in seconds")]
    pub builder_requests_latency: Histogram,
    /// Inbound Requests
    #[metric(describe = "Inbound Requests")]
    pub inbound_requests: Counter,
    /// Total-failure counters partitioned by the runtime fanout label.
    #[metric(skip)]
    fanout_total_failures: FanoutTotalFailures,
    /// Target-health gauges partitioned by runtime fanout and target labels.
    #[metric(skip)]
    fanout_target_health: FanoutTargetHealth,
}

#[derive(Debug)]
struct FanoutTotalFailures;

impl Default for FanoutTotalFailures {
    fn default() -> Self {
        describe_counter!(
            "fanout_total_failures",
            "Fanout requests where every target failed"
        );
        Self
    }
}

impl FanoutTotalFailures {
    fn increment(&self, kind: FanoutKind) {
        counter!("fanout_total_failures", "fanout" => kind.as_str()).increment(1);
    }
}

#[derive(Debug)]
struct FanoutTargetHealth;

impl Default for FanoutTargetHealth {
    fn default() -> Self {
        describe_gauge!(
            "fanout_target_healthy",
            "Whether the latest request to a fanout target succeeded"
        );
        Self
    }
}

impl FanoutTargetHealth {
    fn set(&self, kind: FanoutKind, target: &str, healthy: bool) {
        gauge!(
            "fanout_target_healthy",
            "fanout" => kind.as_str(),
            "target" => target.to_owned(),
        )
        .set(if healthy { 1.0 } else { 0.0 });
    }
}

impl ProxyMetrics {
    /// Records the latency for a request to L2.
    pub fn record_l2_latency(&self, duration: f64) {
        self.l2_requests_latency.record(duration);
    }

    /// Records the latency for a request to the builder.
    pub fn record_builder_latency(&self, duration: f64) {
        self.builder_requests_latency.record(duration);
    }

    /// Records an inbound request.
    pub fn record_inbound_request(&self, value: u64) {
        self.inbound_requests.increment(value);
    }

    pub(crate) fn record_fanout_total_failure(&self, kind: FanoutKind) {
        self.fanout_total_failures.increment(kind);
    }

    pub(crate) fn record_fanout_target_health(
        &self,
        kind: FanoutKind,
        target: &str,
        healthy: bool,
    ) {
        self.fanout_target_health.set(kind, target, healthy);
    }
}
