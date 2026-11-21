fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(4 * 1024 * 1024)
        .enable_all()
        .build()
        .unwrap_or_else(|error| {
            eprintln!("failed to start Tokio runtime: {error}");
            std::process::exit(1);
        });
    if let Err(error) = runtime.block_on(
        dominion_rust::bootstrap::run(std::env::args().skip(1).collect()),
    ) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
