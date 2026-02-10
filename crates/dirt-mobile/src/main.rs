//! Dirt Mobile Application
//!
//! Android shell entrypoint for the Dioxus mobile app.

#[cfg(target_os = "android")]
mod app;
#[cfg(any(target_os = "android", test))]
mod auth;
#[cfg(any(target_os = "android", test))]
mod config;
#[cfg(any(target_os = "android", test))]
mod data;
#[cfg(any(target_os = "android", test))]
mod launch;
#[cfg(any(target_os = "android", test))]
mod secret_store;
#[cfg(any(target_os = "android", test))]
mod sync_auth;
#[cfg(target_os = "android")]
mod ui;

#[cfg(target_os = "android")]
fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dirt=info".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting Dirt mobile shell...");
    dioxus::LaunchBuilder::mobile().launch(app::App);
}

#[cfg(not(target_os = "android"))]
fn main() {
    println!(
        "dirt-mobile is intended for Android targets. Try: cargo build -p dirt-mobile --target aarch64-linux-android"
    );
}
