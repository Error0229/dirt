fn main() {
    // Android phones can run with 16 KB memory pages; NDK 26 links for 4 KB
    // unless told otherwise (see crates/dirt-mobile/build.rs).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-arg=-Wl,-z,max-page-size=16384");
    }
}
