use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Default, Serialize)]
struct DesktopBootstrapConfig {
    dirt_api_base_url: Option<String>,
}

fn main() {
    println!("cargo:rerun-if-env-changed=DIRT_DESKTOP_API_BASE_URL");
    println!("cargo:rerun-if-env-changed=DIRT_API_BASE_URL");
    println!("cargo:rerun-if-changed=../../.env.client");
    println!("cargo:rerun-if-changed=../../.env.client.example");

    if let Err(error) = write_desktop_bootstrap_config() {
        println!("cargo:warning=failed to generate desktop bootstrap config: {error}");
    }
    if let Err(error) = put_voice_folder() {
        panic!("could not put the voice folder in place: {error}");
    }
    // The engine's library is loaded when voice is first used, from the
    // voice folder, not as the program starts (`dirt_voice`): without
    // this the app would not start where it is not beside the program. A
    // build without the engine (`DIRT_VOICE=off`, or offline) has none.
    let engine = env::var("DEP_DIRT_VOICE_ENGINE").as_deref() == Ok("1");
    if engine && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/DELAYLOAD:sherpa-onnx-c-api.dll");
    }
}

/// Puts the voice folder that `dirt-voice`'s build made (the model,
/// Silero's detector and, on Windows, the engine's libraries) at `voice/`
/// beside this manifest, where `asset!` takes it from to ship it with the
/// app. The files are hard links to the build cache, so nothing is kept
/// twice; the folder is not in git. A file the folder no longer has is
/// taken away. A build without voice ships the folder empty, and the app
/// offers no mic.
fn put_voice_folder() -> io::Result<()> {
    println!("cargo:rerun-if-env-changed=DEP_DIRT_VOICE_FOLDER");
    println!("cargo:rerun-if-env-changed=DEP_DIRT_VOICE_ENGINE");
    // Builds of other kinds (another target folder, `DIRT_VOICE=off`) put
    // their own folder here: whichever builds next puts its own back.
    println!("cargo:rerun-if-changed=voice");
    let from = env::var_os("DEP_DIRT_VOICE_FOLDER").map(PathBuf::from);
    let manifest_dir =
        env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let to = manifest_dir.join("voice");
    fs::create_dir_all(&to)?;
    let mut wanted = Vec::new();
    let sources = match &from {
        Some(from) => fs::read_dir(from)?.collect::<Vec<_>>(),
        None => Vec::new(),
    };
    for entry in sources {
        let source = entry?.path();
        let Some(name) = source.file_name() else {
            continue;
        };
        let target = to.join(name);
        let same = fs::metadata(&target)
            .ok()
            .zip(fs::metadata(&source).ok())
            .is_some_and(|(have, want)| have.len() == want.len());
        if !same {
            let _ = fs::remove_file(&target);
            if fs::hard_link(&source, &target).is_err() {
                fs::copy(&source, &target)?;
            }
        }
        wanted.push(name.to_owned());
    }
    for entry in fs::read_dir(&to)? {
        let path = entry?.path();
        if path
            .file_name()
            .is_some_and(|name| !wanted.iter().any(|w| w == name))
        {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn write_desktop_bootstrap_config() -> io::Result<()> {
    load_workspace_dotenv();

    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "OUT_DIR is not set"))?;
    fs::create_dir_all(&out_dir)?;

    // The address is written down as it was given. The app turns the
    // emulator's and `localhost`'s names for this computer into one name
    // when it starts (`dirt_core::config::ServerAddress`).
    let dirt_api_base_url = env_var_trimmed("DIRT_DESKTOP_API_BASE_URL")
        .or_else(|| env_var_trimmed("DIRT_API_BASE_URL"));

    let config = DesktopBootstrapConfig { dirt_api_base_url };

    let content = serde_json::to_string_pretty(&config)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    fs::write(out_dir.join("desktop-bootstrap.json"), content)?;
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
