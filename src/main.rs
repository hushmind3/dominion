mod addons;
mod api;
mod bootstrap;
mod build_identity;
mod config;
mod db;
mod engine;
mod events;
mod migrations;
mod platform;
mod plugin_host;
mod state;
mod vault;

#[tokio::main]
async fn main() {
    if let Err(error) = bootstrap::run(std::env::args().skip(1).collect()).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
