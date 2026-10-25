fn main() {
    println!("cargo:rerun-if-changed=../../assets/brand/videode.ico");
    // The build script runs on the host, so this only fires on Windows hosts.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/brand/videode.ico");
        res.compile().expect("failed to embed the Windows icon");
    }
}
