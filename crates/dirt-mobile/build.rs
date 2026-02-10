use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

const QUICK_CAPTURE_WIDGET_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<appwidget-provider xmlns:android="http://schemas.android.com/apk/res/android"
    android:minWidth="120dp"
    android:minHeight="48dp"
    android:updatePeriodMillis="0"
    android:initialLayout="@android:layout/simple_list_item_1"
    android:resizeMode="horizontal|vertical"
    android:widgetCategory="home_screen" />
"#;

#[derive(Debug, Default, Serialize)]
struct MobileBootstrapConfig {
    supabase_url: Option<String>,
    supabase_anon_key: Option<String>,
    turso_sync_token_endpoint: Option<String>,
}

fn main() {
    println!("cargo:rerun-if-env-changed=WRY_ANDROID_KOTLIN_FILES_OUT_DIR");
    println!("cargo:rerun-if-env-changed=SUPABASE_URL");
    println!("cargo:rerun-if-env-changed=SUPABASE_ANON_KEY");
    println!("cargo:rerun-if-env-changed=TURSO_SYNC_TOKEN_ENDPOINT");

    if let Err(error) = write_mobile_bootstrap_config() {
        println!("cargo:warning=failed to generate mobile bootstrap config: {error}");
    }

    if let Err(error) = write_android_widget_resources() {
        println!("cargo:warning=failed to generate Android widget metadata: {error}");
    }
}

fn write_android_widget_resources() -> io::Result<()> {
    let Some(kotlin_out_dir) = env::var_os("WRY_ANDROID_KOTLIN_FILES_OUT_DIR") else {
        return Ok(());
    };

    let Some(main_dir) = find_android_main_dir(Path::new(&kotlin_out_dir)) else {
        return Ok(());
    };

    let xml_dir = main_dir.join("res").join("xml");
    fs::create_dir_all(&xml_dir)?;
    fs::write(
        xml_dir.join("dirt_quick_capture_widget_info.xml"),
        QUICK_CAPTURE_WIDGET_XML,
    )?;

    Ok(())
}

fn find_android_main_dir(path: &Path) -> Option<PathBuf> {
    path.ancestors().find_map(|ancestor| {
        let parent = ancestor.parent()?;
        if ancestor.file_name() == Some(OsStr::new("main"))
            && parent.file_name() == Some(OsStr::new("src"))
        {
            Some(ancestor.to_path_buf())
        } else {
            None
        }
    })
}

fn write_mobile_bootstrap_config() -> io::Result<()> {
    load_workspace_dotenv();

    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "OUT_DIR is not set"))?;
    fs::create_dir_all(&out_dir)?;

    let config = MobileBootstrapConfig {
        supabase_url: env_var_trimmed("SUPABASE_URL"),
        supabase_anon_key: env_var_trimmed("SUPABASE_ANON_KEY"),
        turso_sync_token_endpoint: env_var_trimmed("TURSO_SYNC_TOKEN_ENDPOINT"),
    };

    let content = serde_json::to_string_pretty(&config)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    fs::write(out_dir.join("mobile-bootstrap.json"), content)?;
    Ok(())
}

fn load_workspace_dotenv() {
    let manifest_dir =
        env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let candidate = manifest_dir.join("..").join("..").join(".env");
    if candidate.exists() {
        let _ = dotenvy::from_path(candidate);
    }
}

fn env_var_trimmed(name: &str) -> Option<String> {
    let value = env::var(name).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
