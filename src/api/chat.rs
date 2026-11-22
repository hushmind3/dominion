use crate::engine;
use crate::state::AppState;
use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use serde_json::Value;
use std::convert::Infallible;
use tokio::sync::mpsc;

pub async fn chat(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let command = match engine::chat::ChatCommand::admit(&state, &body) {
        Ok(command) => command,
        Err(message) => {
            return super::response::error(axum::http::StatusCode::BAD_REQUEST, &message)
        }
    };
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let task_state = state.clone();
    tokio::spawn(async move {
        engine::chat::run_command(task_state, command, sender).await;
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
