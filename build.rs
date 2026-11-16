fn main() {
    println!("cargo:rerun-if-changed=assets/ge-bridge.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/ge-bridge.ico"); // resource ordinal 1
        res.compile().expect("failed to embed icon resource");
    }
}
