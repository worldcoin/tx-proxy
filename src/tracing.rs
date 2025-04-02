use metrics::histogram;
use opentelemetry::trace::Status;

use opentelemetry_sdk::trace::SpanProcessor;
use rollup_boost::SPAN_ATTRIBUTE_LABELS;

/// Custom span processor that records span durations as histograms
#[derive(Debug)]
pub(crate) struct MetricsSpanProcessor;

impl SpanProcessor for MetricsSpanProcessor {
    fn on_start(&self, _span: &mut opentelemetry_sdk::trace::Span, _cx: &opentelemetry::Context) {}

    fn on_end(&self, span: opentelemetry_sdk::trace::SpanData) {
        let duration = span
            .end_time
            .duration_since(span.start_time)
            .unwrap_or_default();

        // Remove status description to avoid cardinality explosion
        let status = match span.status {
            Status::Ok => "ok",
            Status::Error { .. } => "error",
            Status::Unset => "unset",
        };

        // Add custom labels
        let labels = span
            .attributes
            .iter()
            .filter(|attr| SPAN_ATTRIBUTE_LABELS.contains(&attr.key.as_str()))
            .map(|attr| {
                (
                    attr.key.as_str().to_string(),
                    attr.value.as_str().to_string(),
                )
            })
            .chain([
                ("span_kind".to_string(), format!("{:?}", span.span_kind)),
                ("status".to_string(), status.into()),
            ])
            .collect::<Vec<_>>();

        histogram!(format!("{}_duration", span.name), &labels).record(duration);
    }

    fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        Ok(())
    }

    fn shutdown(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        Ok(())
    }
}
