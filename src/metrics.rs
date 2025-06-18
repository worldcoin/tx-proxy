use metrics::{Counter, Histogram, counter, histogram};
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
    /// L2 Failed Requests
    #[metric(describe = "L2 Failed Requests")]
    pub l2_failed_requests: Histogram,
    /// Builder Failed Requests
    #[metric(describe = "Builder Failed Requests")]
    pub builder_failed_requests: Histogram,
    /// Inbound Requests
    #[metric(describe = "Inbound Requests")]
    pub inbound_requests: Counter,
}

impl ProxyMetrics {
    /// Creates a new instance of [`ProxyMetrics`].
    pub fn new() -> Self {
        Self {
            l2_requests_latency: histogram!("l2_requests_latency"),
            builder_requests_latency: histogram!("builder_requests_latency"),
            l2_failed_requests: histogram!("l2_failed_requests"),
            builder_failed_requests: histogram!("builder_failed_requests"),
            inbound_requests: counter!("inbound_requests"),
        }
    }

    /// Records the latency for a request to L2.
    pub fn record_l2_latency(&self, duration: f64) {
        self.l2_requests_latency.record(duration);
    }

    /// Records the latency for a request to the builder.
    pub fn record_builder_latency(&self, duration: f64) {
        self.builder_requests_latency.record(duration);
    }

    /// Records a failed request to L2.
    pub fn record_l2_failed_request(&self, duration: f64) {
        self.l2_failed_requests.record(duration);
    }

    /// Records a failed request to the builder.
    pub fn record_builder_failed_request(&self, duration: f64) {
        self.builder_failed_requests.record(duration);
    }

    /// Records an inbound request.
    pub fn record_inbound_request(&self, value: u64) {
        self.inbound_requests.increment(value);
    }
}
