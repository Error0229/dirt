use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Default, Serialize)]
struct DesktopBootstrapConfig {
    bootstrap_manifest_url: Option<String>,
    supabase_url: Option<String>,
    supabase_anon_key: Option<String>,
    turso_sync_token_endpoint: Option<String>,
    dirt_api_base_url: Option<String>,
}

fn main() {
    println!("cargo:rerun-if-env-changed=SUPABASE_URL");
    println!("cargo:rerun-if-env-changed=SUPABASE_ANON_KEY");
    println!("cargo:rerun-if-env-changed=TURSO_SYNC_TOKEN_ENDPOINT");
    println!("cargo:rerun-if-env-changed=DIRT_API_BASE_URL");
    println!("cargo:rerun-if-env-changed=DIRT_BOOTSTRAP_URL");

    if let Err(error) = write_desktop_bootstrap_config() {
        println!("cargo:warning=failed to generate desktop bootstrap config: {error}");
    }
}

fn write_desktop_bootstrap_config() -> io::Result<()> {
    load_workspace_dotenv();

    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "OUT_DIR is not set"))?;
    fs::create_dir_all(&out_dir)?;

    let dirt_api_base_url = env_var_trimmed("DIRT_API_BASE_URL");
    let bootstrap_manifest_url = env_var_trimmed("DIRT_BOOTSTRAP_URL").or_else(|| {
        dirt_api_base_url
            .as_deref()
            .map(|value| format!("{}/v1/bootstrap", value.trim_end_matches('/')))
    });

    let config = DesktopBootstrapConfig {
        bootstrap_manifest_url,
        supabase_url: env_var_trimmed("SUPABASE_URL"),
        supabase_anon_key: env_var_trimmed("SUPABASE_ANON_KEY"),
        turso_sync_token_endpoint: env_var_trimmed("TURSO_SYNC_TOKEN_ENDPOINT"),
        dirt_api_base_url,
    };

    let content = serde_json::to_string_pretty(&config)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    fs::write(out_dir.join("desktop-bootstrap.json"), content)?;
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
