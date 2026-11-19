// Build scripts must not make moving Git refs inputs to the Rust crate.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for (source, target) in [
        ("DOMINION_RELEASE_COMMIT", "DOMINION_BUILD_COMMIT"),
        ("DOMINION_RELEASE_BRANCH", "DOMINION_BUILD_BRANCH"),
        ("DOMINION_RELEASE_DIRTY", "DOMINION_BUILD_DIRTY"),
    ] {
        println!("cargo:rerun-if-env-changed={source}");
        if let Ok(value) = std::env::var(source) {
            if !value.trim().is_empty() && !value.contains('\n') && !value.contains('\r') {
                println!("cargo:rustc-env={target}={value}");
            }
        }
    }
}
