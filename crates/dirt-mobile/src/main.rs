//! Dirt Mobile Application
//!
//! Android shell entrypoint for the Dioxus mobile app.

#[cfg(target_os = "android")]
mod app;
#[cfg(target_os = "android")]
mod app_shell;
#[cfg(any(target_os = "android", test))]
mod bootstrap_config;
#[cfg(any(target_os = "android", test))]
mod card_files;
#[cfg(any(target_os = "android", test))]
mod config;
#[cfg(any(target_os = "android", test))]
mod data;
#[cfg(any(target_os = "android", test))]
mod draft;
#[cfg(any(target_os = "android", test))]
mod export;
#[cfg(all(target_os = "android", feature = "gallery"))]
mod gallery;
#[cfg(any(target_os = "android", test))]
mod incoming;
#[cfg(target_os = "android")]
mod screen;
#[cfg(target_os = "android")]
mod services;
#[cfg(target_os = "android")]
mod state;
#[cfg(target_os = "android")]
mod ui;
#[cfg(target_os = "android")]
mod views;

#[cfg(target_os = "android")]
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dirt=info".parse().unwrap()),
        )
        .init();

    // Built to show the interface's gallery, this is not the app.
    #[cfg(feature = "gallery")]
    if cfg!(feature = "gallery") {
        dioxus::LaunchBuilder::mobile().launch(gallery::GalleryApp);
        return;
    }

    tracing::info!("Starting Dirt mobile shell...");
    dioxus::LaunchBuilder::mobile().launch(app::App);
}

#[cfg(not(target_os = "android"))]
fn main() {
    println!(
        "dirt-mobile is intended for Android targets. Try: cargo build -p dirt-mobile --target aarch64-linux-android"
    );
}
