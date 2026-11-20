//! Links sherpa-onnx's prebuilt libraries, fetched from the project's
//! release by a pinned SHA-256 into a cache outside `target/` (README.md).
//!
//! It says where they are to the crates that depend on this one directly,
//! as `DEP_SHERPA_ONNX_LIB_DIR`, and whether they are shared, as
//! `DEP_SHERPA_ONNX_SHARED`: `dirt-voice` puts the shared ones beside the
//! model, in the folder the app ships.

#[path = "../build/fetch.rs"]
mod fetch;

use std::path::{Path, PathBuf};

use fetch::{BuildError, Pinned, Wanted};

const RELEASE: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1";

const WINDOWS_X64: Pinned = Pinned {
    name: "sherpa-onnx-v2.0.1-win-x64-shared-MT-Release-lib.tar.bz2",
    url: concat!(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/",
        "sherpa-onnx-v2.0.1-win-x64-shared-MT-Release-lib.tar.bz2"
    ),
    sha256: "afa4de3ca4ab0afd53bb4aab40d28ee15608364b549ffc0fe0caf6489bfc39a5",
};
const ANDROID: Pinned = Pinned {
    name: "sherpa-onnx-v2.0.1-android.tar.bz2",
    url: concat!(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/",
        "sherpa-onnx-v2.0.1-android.tar.bz2"
    ),
    sha256: "39003753ec293425ed56723ea9b972b98841a490af3311670c3060548d654330",
};
const LINUX_X64: Pinned = Pinned {
    name: "sherpa-onnx-v2.0.1-linux-x64-static-lib.tar.bz2",
    url: concat!(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/",
        "sherpa-onnx-v2.0.1-linux-x64-static-lib.tar.bz2"
    ),
    sha256: "55cfadf65ba50d0afc1a802b7a337942819e51c94a5b5a384def1978f7c70871",
};
const MACOS_ARM64: Pinned = Pinned {
    name: "sherpa-onnx-v2.0.1-osx-arm64-static-lib.tar.bz2",
    url: concat!(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/",
        "sherpa-onnx-v2.0.1-osx-arm64-static-lib.tar.bz2"
    ),
    sha256: "d7a9b14f337958e7373c8c650f046df33d210bbaff2f33cb8322dda663ee5e1c",
};
const MACOS_X64: Pinned = Pinned {
    name: "sherpa-onnx-v2.0.1-osx-x64-static-lib.tar.bz2",
    url: concat!(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/",
        "sherpa-onnx-v2.0.1-osx-x64-static-lib.tar.bz2"
    ),
    sha256: "6a3cb3c14104d3a02e7b2a6cd5621f590b14892cce7d0bc0935f7655ef518938",
};

/// The static libraries, in the order the linker needs them.
const STATIC_LIBS: &[&str] = &[
    "sherpa-onnx-c-api",
    "sherpa-onnx-core",
    "kaldi-decoder-core",
    "sherpa-onnx-kaldifst-core",
    "sherpa-onnx-fstfar",
    "sherpa-onnx-fst",
    "kaldi-native-fbank-core",
    "kissfft-float",
    "onnxruntime",
    "ssentencepiece_core",
    "utf8proc",
];

fn main() {
    if let Err(error) = run() {
        panic!("{error}");
    }
}

fn run() -> Result<(), BuildError> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../build/fetch.rs");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    if std::env::var_os("DOCS_RS").is_some() {
        return Ok(());
    }
    let wanted = fetch::wanted()?;
    if wanted == Wanted::Off {
        println!("cargo:absent=1");
        return Ok(());
    }
    let os = std::env::var("CARGO_CFG_TARGET_OS")?;
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH")?;
    let (pin, shared) = match (os.as_str(), arch.as_str()) {
        ("windows", "x86_64") => (WINDOWS_X64, true),
        ("android", _) => (ANDROID, true),
        ("linux", "x86_64") => (LINUX_X64, false),
        ("macos", "aarch64") => (MACOS_ARM64, false),
        ("macos", "x86_64") => (MACOS_X64, false),
        _ => {
            return absent(
                wanted,
                &format!(
                    "sherpa-onnx has no pinned libraries for {os} on {arch}: add the archive \
                     and its hash from {RELEASE} to crates/dirt-voice/sherpa-onnx-sys/build.rs"
                ),
            )
        }
    };
    let found = fetch::unpacked(&pin).and_then(|unpacked| {
        let lib_dir = if os == "android" {
            android_libs(&unpacked, &arch)?
        } else {
            unpacked.join("lib")
        };
        if lib_dir.is_dir() {
            Ok(lib_dir)
        } else {
            Err(format!("{} holds no libraries", lib_dir.display()).into())
        }
    });
    let lib_dir = match found {
        Ok(lib_dir) => lib_dir,
        Err(error) => return absent(wanted, &error.to_string()),
    };

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:lib_dir={}", lib_dir.display());
    println!("cargo:shared={}", u8::from(shared));
    if shared {
        println!("cargo:rustc-link-lib=dylib=sherpa-onnx-c-api");
        println!("cargo:rustc-link-lib=dylib=onnxruntime");
    } else {
        for lib in STATIC_LIBS {
            println!("cargo:rustc-link-lib=static={lib}");
        }
        if os == "linux" {
            for lib in ["stdc++", "m", "pthread", "dl"] {
                println!("cargo:rustc-link-lib=dylib={lib}");
            }
        } else {
            println!("cargo:rustc-link-lib=dylib=c++");
            println!("cargo:rustc-link-lib=framework=Foundation");
        }
    }
    Ok(())
}

/// The engine cannot be had: offline, or on a target with nothing pinned.
/// A build that requires it fails here; any other is built without voice,
/// and says so, and `dirt-voice` offers none (`DEP_SHERPA_ONNX_ABSENT`).
fn absent(wanted: Wanted, why: &str) -> Result<(), BuildError> {
    if wanted == Wanted::Required {
        return Err(format!(
            "voice is required (DIRT_VOICE=required, or a release build) and the speech \
             engine could not be had: {why}. Set DIRT_VOICE=off to build without voice"
        )
        .into());
    }
    println!("cargo:warning=Built without voice: {why}");
    println!("cargo:absent=1");
    Ok(())
}

/// Android's archive holds every ABI, each in `jniLibs/<abi>`.
fn android_libs(unpacked: &Path, arch: &str) -> Result<PathBuf, BuildError> {
    let abi = match arch {
        "aarch64" => "arm64-v8a",
        "arm" => "armeabi-v7a",
        "x86" => "x86",
        "x86_64" => "x86_64",
        _ => return Err(format!("no Android ABI for {arch}").into()),
    };
    [unpacked.join("jniLibs"), unpacked.to_path_buf()]
        .into_iter()
        .map(|folder| folder.join(abi))
        .find(|folder| folder.is_dir())
        .or_else(|| {
            std::fs::read_dir(unpacked).ok()?.find_map(|entry| {
                let candidate = entry.ok()?.path().join("jniLibs").join(abi);
                candidate.is_dir().then_some(candidate)
            })
        })
        .ok_or_else(|| format!("{} has no libraries for {abi}", unpacked.display()).into())
}
