//! HTTP routes for conversations. Handler behavior is unchanged.
use crate::api::{agents, approvals, attachments, chat, personas, rooms, sessions};
use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/attachments", post(attachments::upload))
        .route("/api/attachments/:id/content", get(attachments::content))
        .route("/api/chat", post(chat::chat))
        .route(
            "/api/agents",
            get(agents::list_agents).post(agents::save_agent),
        )
        .route("/api/agents/bulk-model", post(agents::bulk_agent_model))
        .route("/api/personas/import/preview", get(personas::preview))
        .route("/api/personas/import", post(personas::import))
        .route("/api/agents/:id/growth", get(agents::growth))
        .route("/api/agents/:id/avatar", get(personas::avatar))
        .route(
            "/api/agents/:id",
            axum::routing::delete(agents::delete_agent),
        )
        .route("/api/rooms", get(rooms::list).post(rooms::save))
        .route(
            "/api/rooms/:id",
            get(rooms::one).post(rooms::update).delete(rooms::remove),
        )
        .route(
            "/api/rooms/:id/messages",
            get(rooms::messages).post(rooms::send_message),
        )
        .route("/api/rooms/:id/events", get(rooms::events))
        .route("/api/rooms/:id/state", get(rooms::state))
        .route("/api/rooms/:id/knowledge", get(rooms::knowledge))
        .route(
            "/api/rooms/:id/system-prompt",
            get(rooms::system_prompt).post(rooms::save_system_prompt),
        )
        .route("/api/rooms/:id/team-launches", post(rooms::team_launch))
        .route(
            "/api/rooms/:id/team-launches/:launch_id",
            get(rooms::team_launch_status),
        )
        .route("/api/rooms/:id/knowledge/learn", post(rooms::learn))
        .route("/api/rooms/:id/run", post(rooms::run))
        .route("/api/rooms/:id/reset", post(rooms::reset))
        .route("/api/rooms/:id/stop", post(rooms::stop))
        .route(
            "/api/sessions",
            get(sessions::list_sessions).post(sessions::create_session),
        )
        .route(
            "/api/sessions/:id",
            get(sessions::get_session)
                .delete(sessions::delete_session)
                .post(sessions::rename_session),
        )
        .route(
            "/api/sessions/:id/messages",
            get(sessions::session_messages),
        )
        .route("/api/sessions/:id/usage", get(sessions::session_usage))
        .route("/api/sessions/:id/rename", post(sessions::rename_session))
        .route("/api/sessions/:id/clear", post(sessions::clear_session))
        .route("/api/sessions/:id/fork", post(sessions::fork_session))
        .route("/api/sessions/:id/compact", post(sessions::compact_session))
        .route("/api/sessions/:id/cancel", post(sessions::cancel_session))
        .route("/api/sessions/:id/archive", post(sessions::archive_session))
        .route("/api/sessions/:id/learn", post(sessions::learn))
        .route(
            "/api/approvals",
            get(approvals::list_approvals).post(approvals::resolve_approval),
        )
}
