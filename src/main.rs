mod api;
mod build_identity;
mod bootstrap;
mod config;
mod db;
mod engine;
mod events;
mod integrations;
mod migrations;
mod platform;
mod state;
mod vault;
#[cfg(test)]
mod test_support;

#[tokio::main]
async fn main() {
    if let Err(error) = bootstrap::run(std::env::args().skip(1).collect()).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
