use crate::prelude::*;

use tracing::span::Attributes;
use tracing::subscriber::Interest;
use tracing::Event;
use tracing::Id;
use tracing::Metadata;
use tracing::Subscriber;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Registry;

use tracing_subscriber::prelude::*;

pub struct MyLayer;

impl<S: Subscriber + Debug + for<'a> LookupSpan<'a>> tracing_subscriber::Layer<S> for MyLayer {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if metadata.module_path().is_some_with(|path| {
            path.starts_with("ide_ci::")
                || path.starts_with("enso_build")
                || path.starts_with("enso_build2")
        }) {
            Interest::always()
        } else {
            // dbg!(metadata);
            Interest::never()
        }
    }

    fn on_enter(&self, _id: &Id, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        // ide_ci::global::println(format!("Enter {id:?}"));
    }
    fn on_exit(&self, _id: &Id, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        // ide_ci::global::println(format!("Leave {id:?}"));
    }
    fn on_event(&self, _event: &Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        // tracing_log::dbg!(event);
    }
    fn on_new_span(
        &self,
        _attrs: &Attributes<'_>,
        id: &Id,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let span = ctx.span(id).unwrap();
        let bar = crate::global::new_spinner(format!("In span {id:?}: {:?}", span.name()));
        span.extensions_mut().insert(bar);
        // crate::global::println(format!("Create {id:?}"));
    }

    fn on_close(&self, _id: Id, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        // crate::global::println(format!("Close {id:?}"));
    }
}


pub fn setup_logging() -> Result {
    tracing::subscriber::set_global_default(
        Registry::default().with(MyLayer).with(
            tracing_subscriber::fmt::layer()
                .without_time()
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE),
        ),
    )
    .anyhow_err()
}