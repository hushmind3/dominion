use crate::engine;
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response,sse::{Event,KeepAlive,Sse}},Json};
use serde_json::{json,Value};
use super::response::error;
use std::convert::Infallible;
use tokio::sync::mpsc;

pub async fn chat(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    // Snapshot admission before spawning: STOP may arrive while scheduling
    // this turn, before the native run has acquired its session lease.
    // Passing the epoch prevents that older turn clearing a newer STOP.
    let admitted_epoch = body["sessionId"].as_str()
        .map(|session| state.cancellation_generation(session));
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let task_state = state.clone();
    tokio::spawn(async move {
        engine::chat::run_with_http_epoch(task_state, body, sender, admitted_epoch).await;
    });

    let stream = async_stream::stream! {
        while let Some(event) = receiver.recv().await {
            yield Ok::<_, Infallible>(Event::default().data(event.to_string()));
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}
