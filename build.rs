fn main() {
    #[cfg(target_os = "windows")]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            println!("cargo:rerun-if-changed=build.rs");
        }
    }
}
