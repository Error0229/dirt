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

/// The one folder the app's file provider may lend a file from: where a
/// note's file is copied to be opened by another app. `files-path` is the
/// app's own files folder, and `dirt/opened/` is the folder of copies
/// under the app's data folder (`services::files::opened_folder`).
const FILE_PATHS_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<paths>
    <files-path name="opened" path="dirt/opened/" />
</paths>
"#;

#[derive(Debug, Default, Serialize)]
struct MobileBootstrapConfig {
    dirt_api_base_url: Option<String>,
}

fn main() {
    // Android 15 and later can run with memory pages of 16 KB, and some
    // phones do (a Pixel 10 showed it first). A library whose segments are
    // aligned to 4 KB, which is what NDK 26 links by default, is loaded
    // there only by a slower compatibility path, with a warning on screen
    // in a debuggable app, and Google Play refuses it. Aligned to 16 KB it
    // loads on either kind of phone.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-arg=-Wl,-z,max-page-size=16384");
    }
    println!("cargo:rerun-if-env-changed=WRY_ANDROID_KOTLIN_FILES_OUT_DIR");
    println!("cargo:rerun-if-env-changed=DIRT_MOBILE_API_BASE_URL");
    println!("cargo:rerun-if-env-changed=DIRT_API_BASE_URL");
    println!("cargo:rerun-if-changed=../../.env.client");
    println!("cargo:rerun-if-changed=../../.env.client.example");

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
    fs::write(xml_dir.join("dirt_file_paths.xml"), FILE_PATHS_XML)?;

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

    // The address is written down as it was given. The app turns this
    // computer's names for itself into the emulator's name for it when it
    // starts (`dirt_core::config::ServerAddress::on_android`).
    let dirt_api_base_url = env_var_trimmed("DIRT_MOBILE_API_BASE_URL")
        .or_else(|| env_var_trimmed("DIRT_API_BASE_URL"));

    let config = MobileBootstrapConfig { dirt_api_base_url };

    let content = serde_json::to_string_pretty(&config)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    fs::write(out_dir.join("mobile-bootstrap.json"), content)?;
    Ok(())
}

fn load_workspace_dotenv() {
    let manifest_dir =
        env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let workspace_root = manifest_dir.join("..").join("..");

    // Load role-separated client environment values for bootstrap generation.
    let client_env = workspace_root.join(".env.client");

    if client_env.exists() {
        let _ = dotenvy::from_path(client_env);
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
