//! Embeds the icon and version info into the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=assets/recrec.ico");
    // The resource compiler only exists on Windows hosts; cross-checks elsewhere skip it.
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/recrec.ico")
                .set("ProductName", "recrec")
                .set("FileDescription", "recrec - multichannel recorder");
            if let Err(e) = res.compile() {
                panic!("failed to embed Windows resources: {e}");
            }
        }
    }
}
