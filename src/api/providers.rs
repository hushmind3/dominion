//! Provider API boundary: credentials, sign-in, public catalogs and saved records
//! have separate owners. Public handler names remain stable for callers.
mod catalog;
mod credentials;
mod models;
mod oauth;
mod pricing;
mod probe;
mod records;
#[cfg(test)]
mod tests;

pub use catalog::{
    free_catalog, free_catalog_refresh, provider_catalog, public_catalog_models,
    public_openrouter_free_catalog, refresh_free_catalog,
};
pub use credentials::{cli_credentials, import_cli_credential, set_cli_model};
pub use models::provider_models;
pub use oauth::{codex_device_start, codex_login_start, codex_login_status};
pub use probe::test_provider;
pub use records::{delete_provider, list_providers, save_provider};
