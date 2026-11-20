//! Puts together the voice folder: what the apps ship so that speech can
//! be turned into text on the device (interface design, 6.18).
//!
//! - The speech model, Parakeet TDT-CTC 110M (CC BY 4.0, NVIDIA), as
//!   sherpa-onnx's project publishes it converted to ONNX at 8 bits.
//! - Silero's voice-activity detector, which finds the pauses.
//! - On Windows, the engine's two libraries, which the program loads from
//!   this folder when it first listens or reads.
//! - `NOTICE.txt`, the credits the licences ask for.
//!
//! Each file is fetched once, by a pinned address and SHA-256, into the
//! build cache outside `target/` (`build/fetch.rs`), and the folder is made
//! of hard links to them there. The apps' build scripts hear where it is as
//! `DEP_DIRT_VOICE_FOLDER`, and this crate's tests as
//! `DIRT_VOICE_BUILT_FOLDER`.
//!
//! `DIRT_VOICE_MODEL=skip` leaves the model out, for a build that is
//! checked or tested and not shipped: CI's jobs but one (the model is
//! 130 MiB). The folder then has the detector and the libraries, and
//! [`crate::VoiceFolder::has_model`] says no.
//!
//! `DIRT_VOICE=off` builds without the engine, fetching nothing; so does a
//! development build that cannot fetch it or its files, saying why in a
//! warning. A release build, or `DIRT_VOICE=required`, fails instead
//! (`build/fetch.rs`, `Wanted`). Without the engine `voice_engine` is not
//! set and the crate's code says voice is not in this build.

#[path = "build/fetch.rs"]
mod fetch;

use std::fs;
use std::path::{Path, PathBuf};

use fetch::{BuildError, Pinned, Wanted};

const MODEL: Pinned = Pinned {
    name: "sherpa-onnx-nemo-parakeet_tdt_transducer_110m-en-36000-int8.tar.bz2",
    url: concat!(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/",
        "sherpa-onnx-nemo-parakeet_tdt_transducer_110m-en-36000-int8.tar.bz2"
    ),
    sha256: "f628312e9fdf8686374cb01a69425c41732529d540860311f16f37cbc32cfe9b",
};

const DETECTOR: Pinned = Pinned {
    name: "silero_vad.onnx",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
    sha256: "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6",
};

/// The model's files, as its archive names them.
const MODEL_FILES: [&str; 4] = [
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joiner.int8.onnx",
    "tokens.txt",
];

fn main() {
    if let Err(error) = run() {
        panic!("{error}");
    }
}

fn run() -> Result<(), BuildError> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/fetch.rs");
    println!("cargo:rerun-if-changed=NOTICE.txt");
    println!("cargo:rerun-if-env-changed=DIRT_VOICE_MODEL");
    // `voice_engine` is set when the engine is linked: without it the
    // crate builds, and listening and reading say voice is not in this
    // build (`DIRT_VOICE=off`, offline, or a target with nothing pinned).
    println!("cargo:rustc-check-cfg=cfg(voice_engine)");
    // Always set, so that `env!` finds it; empty without the model.
    let wanted = fetch::wanted()?;
    let engine = std::env::var_os("DEP_SHERPA_ONNX_ABSENT").is_none();
    let made = if wanted == Wanted::Off || !engine {
        Err(None)
    } else {
        put_folder().map_err(Some)
    };
    match made {
        Ok((folder, model)) => {
            println!("cargo:rustc-cfg=voice_engine");
            println!("cargo:engine=1");
            println!("cargo:folder={}", folder.display());
            println!(
                "cargo:rustc-env=DIRT_VOICE_BUILT_FOLDER={}",
                folder.display()
            );
            println!("cargo:rustc-env=DIRT_VOICE_BUILT_MODEL={model}");
        }
        Err(error) => {
            if let (Some(error), Wanted::Required) = (&error, wanted) {
                return Err(format!(
                    "voice is required (DIRT_VOICE=required, or a release build) and its \
                     files could not be had: {error}. Set DIRT_VOICE=off to build without voice"
                )
                .into());
            }
            if let Some(error) = error {
                println!("cargo:warning=Built without voice: {error}");
            }
            println!("cargo:engine=0");
            println!("cargo:rustc-env=DIRT_VOICE_BUILT_FOLDER=");
            println!("cargo:rustc-env=DIRT_VOICE_BUILT_MODEL=");
        }
    }
    Ok(())
}

/// Puts the voice folder together: where it is, and where the model was
/// unpacked (empty without it).
fn put_folder() -> Result<(PathBuf, String), BuildError> {
    let target = std::env::var("TARGET")?;
    let os = std::env::var("CARGO_CFG_TARGET_OS")?;
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let lib_dir = std::env::var_os("DEP_SHERPA_ONNX_LIB_DIR").map(PathBuf::from);
    let shared = std::env::var("DEP_SHERPA_ONNX_SHARED").as_deref() == Ok("1");
    let skip_model = std::env::var("DIRT_VOICE_MODEL").as_deref() == Ok("skip");

    let detector = fetch::fetched(&DETECTOR)?;
    let model = if skip_model {
        None
    } else {
        Some(fetch::unpacked(&MODEL)?)
    };

    let name = if model.is_some() {
        format!("{target}-{}", &MODEL.sha256[..12])
    } else {
        format!("{target}-without-model")
    };
    let folder = fetch::cache_root().join("voice").join(name);
    fs::create_dir_all(&folder)?;
    fetch::link_or_copy(&detector, &folder.join(DETECTOR.name))?;
    fs::copy("NOTICE.txt", folder.join("NOTICE.txt"))?;
    if let Some(model) = &model {
        for file in MODEL_FILES {
            let source = model.join(file);
            if !source.is_file() {
                return Err(format!("the model has no {}", source.display()).into());
            }
            fetch::link_or_copy(&source, &folder.join(file))?;
        }
    }
    // The engine's libraries go with the model where they are loaded at
    // run time: on Windows from this folder, on Android from the app's.
    if shared {
        if let Some(lib_dir) = &lib_dir {
            put_libraries(lib_dir, &folder, &os)?;
        }
    }

    let model = model
        .as_deref()
        .map(Path::display)
        .map(|path| path.to_string())
        .unwrap_or_default();

    // On Windows the engine's libraries are loaded when they are first
    // used, from the voice folder, and not when the program starts: a
    // program that could not find them would not start at all. Every
    // program that links this crate says so to the linker; the apps'
    // build scripts do, and this one does for the crate's own tests.
    if msvc && shared {
        println!("cargo:rustc-link-lib=delayimp");
        println!("cargo:rustc-link-arg=/DELAYLOAD:sherpa-onnx-c-api.dll");
    }
    Ok((folder, model))
}

/// The shared libraries the program loads, beside the model.
fn put_libraries(lib_dir: &Path, folder: &Path, os: &str) -> Result<(), BuildError> {
    let extension = match os {
        "windows" => "dll",
        "android" => "so",
        _ => return Ok(()),
    };
    // Not the C++ interface, which nothing here calls.
    let wanted = |path: &Path, name: &str| {
        path.extension()
            .is_some_and(|found| found.eq_ignore_ascii_case(extension))
            && !name.contains("cxx")
    };
    for entry in fs::read_dir(lib_dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if wanted(&path, name) {
            fetch::link_or_copy(&path, &folder.join(name))?;
        }
    }
    Ok(())
}
