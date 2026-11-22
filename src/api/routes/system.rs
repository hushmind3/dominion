//! HTTP routes for system. Handler behavior is unchanged.
use crate::api::{agents, approvals, compat, dev, extensions, settings, system, translation};
use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(system::health))
        .route("/api/status", get(system::status))
        .route(
            "/api/settings/assistant",
            get(settings::assistant_settings).post(settings::save_assistant_settings),
        )
        .route(
            "/api/settings/system-prompt/preview",
            get(settings::system_prompt_preview),
        )
        .route(
            "/api/settings/system-prompt/override",
            post(settings::save_system_prompt_override),
        )
        .route(
            "/api/settings/owner-profile",
            get(settings::owner_profile).post(settings::save_owner_profile),
        )
        .route("/api/events", get(system::events))
        .route("/api/model-usage", get(system::model_usage))
        .route("/api/policies", get(system::policies))
        .route("/api/system/backup", post(system::create_backup))
        .route("/api/system/backup-keys", get(system::backup_keys))
        .route(
            "/api/system/backup-keys/rotate",
            post(system::rotate_backup_keys),
        )
        .route("/api/system/secret-key", get(system::secret_key))
        .route(
            "/api/system/secret-key/rotate",
            post(system::rotate_secret_key),
        )
        .route("/api/system/backups", get(system::list_backups))
        .route(
            "/api/system/maintenance",
            get(system::maintenance_status).post(system::maintenance),
        )
        .route("/api/system/restore", post(system::restore_backup))
        .route(
            "/api/extensions/runtime",
            get(extensions::runtime_extensions).post(extensions::set_runtime_extensions),
        )
        .route(
            "/api/policy",
            get(approvals::get_policy).post(approvals::save_policy),
        )
        .route("/api/audit", get(system::audit_log))
        .route("/api/model-route", get(system::model_route))
        .route("/api/translate", post(translation::translate))
        .route("/api/translate/config", get(translation::translate_config))
        .route("/api/upload", post(compat::upload))
        .route("/api/synergy", get(agents::synergy))
        .route("/api/capabilities", get(system::capabilities))
        .route("/api/commands", get(system::commands))
        .route("/api/skill-catalog", get(extensions::skill_catalog))
        .route(
            "/api/skill-catalog/meta",
            get(extensions::skill_catalog_meta),
        )
        .route("/api/skill-catalog/import", post(extensions::import_skill))
        .route("/api/dev/jobs/:id", get(dev::get_job))
        .route("/api/dev/update", get(dev::update_status).post(dev::update))
        .route("/api/system/pick-folder", post(compat::pick_folder))
}
