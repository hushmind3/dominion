mod api;
mod build_identity;
mod bootstrap;
mod config;
mod engine;
mod host;
mod conversation;
mod events;
mod features;
mod integrations;
mod platform;
mod storage;
pub(crate) use crate::host::state;
pub(crate) use crate::storage::{sqlite as db, migrations, vault};
#[cfg(test)]
mod test_support;

#[tokio::main]
async fn main() {
    if let Err(error) = bootstrap::run(std::env::args().skip(1).collect()).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
