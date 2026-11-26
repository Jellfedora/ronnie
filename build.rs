fn main() {
    // The updater picks the release archive built for this target.
    println!("cargo:rustc-env=TARGET={}", std::env::var("TARGET").unwrap());

    // Windows: icon and version info in the .exe.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/ronnie.ico");
        // The name Windows shows (Task Manager, « Open with »…), not the package's lowercase one.
        res.set("ProductName", "Ronnie");
        res.set("FileDescription", "Ronnie");
        res.compile().expect("compile Windows resources");
    }
    println!("cargo:rerun-if-changed=build.rs");
    // Set by scripts/package.sh for the published builds (see config::OFFICIAL).
    println!("cargo:rerun-if-env-changed=RONNIE_OFFICIAL");
    println!("cargo:rerun-if-changed=assets/icon/ronnie.ico");
}
