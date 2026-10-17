//! Per-future recording layer; tests never install a global subscriber or recorder.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::{Layer, layer::Context, prelude::*, registry::LookupSpan};

/// One event and its inherited safe operation correlation fields.
#[derive(Clone, Debug, Default)]
pub(crate) struct Observation(pub BTreeMap<String, String>);

impl Visit for Observation {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
}

/// Independently owned observer for one test's futures.
#[derive(Clone, Default)]
pub(crate) struct RecordingObserver(Arc<Mutex<Vec<Observation>>>);

impl RecordingObserver {
    /// Construct a local subscriber, without changing process-wide tracing state.
    pub(crate) fn subscriber(self) -> impl Subscriber + Send + Sync {
        tracing_subscriber::registry().with(self)
    }

    /// Return detached observations for assertions.
    pub(crate) fn observations(&self) -> Vec<Observation> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }
}

impl<S> Layer<S> for RecordingObserver
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        ctx: Context<'_, S>,
    ) {
        let mut fields = Observation::default();
        attrs.record(&mut fields);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut observation = Observation::default();
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(fields) = span.extensions().get::<Observation>() {
                    observation.0.extend(fields.0.clone());
                }
            }
        }
        event.record(&mut observation);
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push(observation);
    }
}
