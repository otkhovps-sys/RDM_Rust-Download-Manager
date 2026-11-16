//! Gives `rdm.exe` its icon and version details on Windows.

fn main() {
    println!("cargo:rerun-if-changed=assets/rdm.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/rdm.ico")
            .set("ProductName", "RDM - Rust Download Manager")
            .set("FileDescription", "RDM - Rust Download Manager");
        if let Err(e) = resource.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
