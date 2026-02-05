//! Build script to set Windows stack size

fn main() {
    // Set a larger stack size on Windows (8MB instead of default 1MB)
    // Required because libsql has deep call stacks during sync operations
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-arg=/STACK:8388608");
    }
}
