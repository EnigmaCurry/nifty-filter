use axum::{
    extract::State,
    response::sse::{Event, Sse},
};
use futures_util::stream::{Stream, StreamExt};
use std::{convert::Infallible, time::Duration};
use tokio_stream::wrappers::BroadcastStream;
use tracing::info;

use crate::AppState;

pub async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    info!("SSE client connected to /api/events");
    let config_rx = state.config_changed_tx.subscribe();
    let shutdown_rx = state.shutdown_tx.subscribe();

    let config_stream = BroadcastStream::new(config_rx).filter_map(|result| async move {
        match result {
            Ok(()) => Some(Ok(Event::default().event("config-changed").data("reload"))),
            Err(_) => None,
        }
    });

    let shutdown_stream = BroadcastStream::new(shutdown_rx).filter_map(|result| async move {
        match result {
            Ok(()) => Some(Ok(Event::default().event("shutdown").data("goodbye"))),
            Err(_) => None,
        }
    });

    let stream = futures_util::stream::select(config_stream, shutdown_stream);

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text(""),
    )
}
