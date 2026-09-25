fn main() {
    // The updater picks the release archive built for this target.
    println!("cargo:rustc-env=TARGET={}", std::env::var("TARGET").unwrap());

    // Windows: icon and version info in the .exe.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/ronnie.ico");
        res.compile().expect("compile Windows resources");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon/ronnie.ico");
}
