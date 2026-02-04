//! Dirt Desktop Application
//!
//! A cross-platform desktop app for capturing fleeting thoughts.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod components;
mod services;
mod state;
mod theme;
mod views;

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("dirt=debug".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting Dirt...");

    dioxus::launch(app::App);
}
