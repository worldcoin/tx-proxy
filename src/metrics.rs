use metrics::{Counter, Histogram, counter, describe_counter, describe_gauge, histogram};
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
}

impl ProxyMetrics {
    /// Creates a new instance of [`ProxyMetrics`].
    pub fn new() -> Self {
        let metrics = Self {
            l2_requests_latency: histogram!("l2_requests_latency"),
            builder_requests_latency: histogram!("builder_requests_latency"),
            inbound_requests: counter!("inbound_requests"),
        };

        describe_counter!(
            "fanout_total_failures",
            "Fanout requests where every target failed"
        );
        describe_gauge!(
            "fanout_target_healthy",
            "Whether the latest request to a fanout target succeeded"
        );

        metrics
    }

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
}
