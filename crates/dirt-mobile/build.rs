use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

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
    println!("cargo:rerun-if-env-changed=DIRT_MOBILE_API_BASE_URL");
    println!("cargo:rerun-if-env-changed=DIRT_API_BASE_URL");
    println!("cargo:rerun-if-changed=../../.env.client");
    println!("cargo:rerun-if-changed=../../.env.client.example");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        if let Err(error) = put_voice_files() {
            panic!("could not put the voice files in place: {error}");
        }
    }

    if let Err(error) = write_mobile_bootstrap_config() {
        println!("cargo:warning=failed to generate mobile bootstrap config: {error}");
    }
}

/// Puts what `dirt-voice`'s build made into the native module
/// (`android/native`), which `dx` copies into the Gradle project, so that
/// the APK carries it (interface design 8.9):
///
/// - the engine's libraries in `src/main/jniLibs/<ABI>/`, which the app's
///   own library is linked against;
/// - the model, Silero's detector and the notice in `src/main/assets/voice/`,
///   left compressed in the APK and copied out once, on first use, into
///   the app's folder that is not backed up (`VoiceFiles.kt`): the engine
///   opens its files by path, and a file inside the APK has none, so it is
///   copied out whether it is stored compressed or not;
/// - `src/main/res/values/dirt_voice.xml`, which says whether this build
///   has voice, for what the manifest offers only with it (Talk).
///
/// Hard links to the build cache where it can, copies where it cannot;
/// none of it is in git. Each is made again when what it was made from
/// changed (its size or its time, recorded in `OUT_DIR`), so a new pin of
/// the same size is not missed. A build without voice empties both
/// folders, and the app offers no mic.
#[allow(clippy::too_many_lines)]
fn put_voice_files() -> io::Result<()> {
    println!("cargo:rerun-if-env-changed=DEP_DIRT_VOICE_FOLDER");
    println!("cargo:rerun-if-env-changed=DEP_DIRT_VOICE_ENGINE");
    let abi = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64-v8a",
        Ok("x86_64") => "x86_64",
        Ok("arm") => "armeabi-v7a",
        Ok("x86") => "x86",
        _ => return Ok(()),
    };
    let manifest_dir =
        env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let main = manifest_dir
        .join("android")
        .join("native")
        .join("src")
        .join("main");
    let libs = main.join("jniLibs").join(abi);
    let assets = main.join("assets").join("voice");
    println!("cargo:rerun-if-changed={}", libs.display());
    println!("cargo:rerun-if-changed={}", assets.display());
    fs::create_dir_all(&libs)?;
    fs::create_dir_all(&assets)?;
    let from = env::var_os("DEP_DIRT_VOICE_FOLDER").map(PathBuf::from);
    let sources: Vec<PathBuf> = match &from {
        Some(from) if env::var("DEP_DIRT_VOICE_ENGINE").as_deref() == Ok("1") => {
            fs::read_dir(from)?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                // The engine's Java bindings: the app calls its C API.
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_none_or(|name| !name.contains("-jni"))
                })
                .collect()
        }
        _ => Vec::new(),
    };
    let is_library = |path: &Path| path.extension().is_some_and(|ending| ending == "so");
    // What each file in the two folders was made from, as it was then.
    let record = env::var_os("OUT_DIR")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("voice-files.txt");
    let before = fs::read_to_string(&record).unwrap_or_default();
    let mut now = String::new();
    let identity = |path: &Path| -> String {
        fs::metadata(path).map_or_else(
            |_| String::new(),
            |meta| {
                let time = meta
                    .modified()
                    .ok()
                    .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |at| at.as_nanos());
                format!("{} {time}", meta.len())
            },
        )
    };
    for (folder, wanted) in [(&libs, true), (&assets, false)] {
        let names: Vec<_> = sources
            .iter()
            .filter(|source| is_library(source) == wanted)
            .filter_map(|source| source.file_name().map(|name| (source, name.to_owned())))
            .collect();
        for (source, name) in &names {
            let target = folder.join(name);
            let line = format!("{} {}\n", target.display(), identity(source));
            let same = target.is_file() && before.lines().any(|seen| format!("{seen}\n") == line);
            now.push_str(&line);
            if !same {
                let _ = fs::remove_file(&target);
                if fs::hard_link(source, &target).is_err() {
                    fs::copy(source, &target)?;
                }
            }
        }
        for entry in fs::read_dir(folder)? {
            let path = entry?.path();
            let known = path
                .file_name()
                .is_some_and(|found| names.iter().any(|(_, name)| name == found));
            if !known {
                fs::remove_file(path)?;
            }
        }
    }
    fs::write(&record, now)?;
    // Voice is in this build when the detector and the model came with the
    // engine: the same that `VoiceFiles.shipped` asks of the assets.
    let shipped = [
        "silero_vad.onnx",
        "encoder.int8.onnx",
        "decoder.int8.onnx",
        "joiner.int8.onnx",
        "tokens.txt",
    ]
    .iter()
    .all(|wanted| {
        sources
            .iter()
            .any(|source| source.file_name() == Some(OsStr::new(wanted)))
    });
    let values = main.join("res").join("values");
    fs::create_dir_all(&values)?;
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <!-- Made by dirt-mobile's build.rs: whether this build has voice. Not in git. -->\n\
         <resources>\n    <bool name=\"dirt_voice_shipped\">{shipped}</bool>\n</resources>\n"
    );
    let file = values.join("dirt_voice.xml");
    if fs::read_to_string(&file).ok().as_deref() != Some(xml.as_str()) {
        fs::write(file, xml)?;
    }
    Ok(())
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
