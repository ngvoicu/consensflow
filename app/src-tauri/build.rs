fn main() {
    tauri_build::build();
    // A test binary on Windows gets no manifest from cargo, and one is needed:
    // see tests.manifest. Only the tests; the app has Tauri's.
    println!("cargo:rerun-if-changed=tests.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = std::env::current_dir()
            .expect("build directory")
            .join("tests.manifest");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
