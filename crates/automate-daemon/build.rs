fn main() {
    println!(
        "cargo:rustc-env=BUILD_VERSION={}",
        std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".to_string())
    );
}
