//! Product-domain HTTP endpoints, outside the minimal turn API.
//! Shared root router applies auth, CORS, maintenance and fallback.
use super::super::*;
pub(super) fn routes()->Router<AppState>{
    Router::<AppState>::new()
        .route("/api/status", get(system::status))
        .route(
            "/api/settings/assistant",
            get(settings::assistant_settings).post(settings::save_assistant_settings),
        )
        .route("/api/settings/system-prompt/preview",get(settings::system_prompt_preview))
        .route("/api/settings/system-prompt/override",post(settings::save_system_prompt_override))
        .route(
            "/api/settings/owner-profile",
            get(settings::owner_profile).post(settings::save_owner_profile),
        )
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
        .route("/api/audit", get(system::audit_log))
        .route("/api/capabilities", get(system::capabilities))
        .route("/api/commands", get(system::commands))
        .route("/api/dev/jobs/:id", get(dev::get_job))
        .route("/api/dev/update", get(dev::update_status).post(dev::update))
        .route("/api/system/pick-folder", post(compat::pick_folder))
}
