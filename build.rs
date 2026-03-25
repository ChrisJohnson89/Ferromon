fn main() {
    let version = std::env::var("FERRO_RELEASE_VERSION")
        .unwrap_or_else(|_| std::env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo:rustc-env=FERRO_VERSION={version}");
    println!("cargo:rerun-if-env-changed=FERRO_RELEASE_VERSION");
}
