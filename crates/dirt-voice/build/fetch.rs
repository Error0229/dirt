//! Fetching what the voice engine is built with: a file at a pinned
//! address, checked against the SHA-256 written here before it is used,
//! and kept in a cache outside `target/` so that `cargo clean` does not
//! fetch it again.
//!
//! Shared by two build scripts: `dirt-voice`'s (the speech model and
//! Silero's detector) and the vendored `sherpa-onnx-sys`'s (the engine's
//! prebuilt libraries). Each includes this file with `#[path]`.
//!
//! The cache is the folder named by `DIRT_BUILD_CACHE`, or else the
//! system's place for caches: `%LOCALAPPDATA%\dirt\build-cache` on
//! Windows, `~/Library/Caches/dirt-build` on macOS and
//! `$XDG_CACHE_HOME/dirt-build` (or `~/.cache/dirt-build`) elsewhere. A
//! file is fetched into it under a name of its own and renamed into place
//! only once its hash is right, and an archive is unpacked beside it the
//! same way, so two builds at once, from two checkouts, never see half a
//! file. An archive is removed once unpacked: one copy of each is kept.

#![allow(dead_code)]

use std::error::Error;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

pub type BuildError = Box<dyn Error>;

/// A file to fetch, and what it must hash to.
pub struct Pinned {
    /// The file's name, as it is kept in the cache.
    pub name: &'static str,
    pub url: &'static str,
    /// The SHA-256 of the file, in lower-case hex.
    pub sha256: &'static str,
}

impl Pinned {
    /// The name without `.tar.bz2`: the folder it unpacks into.
    pub fn stem(&self) -> &'static str {
        self.name.strip_suffix(".tar.bz2").unwrap_or(self.name)
    }
}

/// What the build was asked for: `DIRT_VOICE=off` builds without voice,
/// fetching nothing; `DIRT_VOICE=required` fails when the engine or its
/// files cannot be had. Unset, a release build requires voice, since it is
/// meant to ship with it, and any other build goes without it, saying why
/// in a warning, when it cannot be had: offline, or on a target with no
/// pinned libraries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    Off,
    IfItCanBeHad,
    Required,
}

pub fn wanted() -> Result<Wanted, BuildError> {
    println!("cargo:rerun-if-env-changed=DIRT_VOICE");
    match std::env::var("DIRT_VOICE").unwrap_or_default().trim() {
        "off" => Ok(Wanted::Off),
        "required" => Ok(Wanted::Required),
        "" | "auto" => Ok(if std::env::var("PROFILE").as_deref() == Ok("release") {
            Wanted::Required
        } else {
            Wanted::IfItCanBeHad
        }),
        other => Err(format!("DIRT_VOICE={other}: it takes off, required or auto").into()),
    }
}

/// How long a download may wait to connect, and for each read.
const CONNECT_WITHIN: Duration = Duration::from_secs(20);
const READ_WITHIN: Duration = Duration::from_secs(60);

/// Something made by a build that stopped before it was done is taken away
/// once it is this old: no build takes so long over one file.
const LEFT_BEHIND_AFTER: Duration = Duration::from_secs(3600);

/// Takes away what builds that stopped half way left in `folder`: files
/// and folders named `….part-<process>`, older than an hour.
fn clear_left_behind(folder: &Path) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let is_part = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(".part-"));
        let old = entry
            .metadata()
            .and_then(|found| found.modified())
            .ok()
            .and_then(|at| SystemTime::now().duration_since(at).ok())
            .is_some_and(|age| age > LEFT_BEHIND_AFTER);
        if is_part && old {
            let _ = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
        }
    }
}

/// Takes a part away if the work that made it does not finish.
struct Part(PathBuf);

impl Drop for Part {
    fn drop(&mut self) {
        if self.0.is_dir() {
            let _ = fs::remove_dir_all(&self.0);
        } else {
            let _ = fs::remove_file(&self.0);
        }
    }
}

/// The folder everything fetched is kept in.
pub fn cache_root() -> PathBuf {
    println!("cargo:rerun-if-env-changed=DIRT_BUILD_CACHE");
    if let Some(folder) = std::env::var_os("DIRT_BUILD_CACHE").filter(|f| !f.is_empty()) {
        return PathBuf::from(folder);
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("dirt").join("build-cache");
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = home() {
            return home.join("Library").join("Caches").join("dirt-build");
        }
    } else if let Some(cache) = std::env::var_os("XDG_CACHE_HOME").filter(|f| !f.is_empty()) {
        return PathBuf::from(cache).join("dirt-build");
    } else if let Some(home) = home() {
        return home.join(".cache").join("dirt-build");
    }
    std::env::temp_dir().join("dirt-build")
}

/// A name for something being made by this process only.
fn ours(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".part-{}", std::process::id()));
    path.with_file_name(name)
}

fn sha256_of(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// The file, fetched into the cache if it is not there, and checked.
///
/// A file in the cache is trusted when the note of its hash beside it says
/// the pinned one: it was checked when it came.
pub fn fetched(pin: &Pinned) -> Result<PathBuf, BuildError> {
    let folder = cache_root().join("downloads");
    fs::create_dir_all(&folder)?;
    let path = folder.join(pin.name);
    let checked = folder.join(format!("{}.sha256", pin.name));
    if path.is_file() && fs::read_to_string(&checked).is_ok_and(|seen| seen.trim() == pin.sha256) {
        return Ok(path);
    }
    clear_left_behind(&folder);
    let part = Part(ours(&path));
    eprintln!("Fetching {} from {}", pin.name, pin.url);
    let response = ureq::builder()
        .try_proxy_from_env(true)
        .timeout_connect(CONNECT_WITHIN)
        .timeout_read(READ_WITHIN)
        .build()
        .get(pin.url)
        .call()
        .map_err(|error| format!("could not fetch {}: {error}", pin.url))?;
    {
        let mut out = File::create(&part.0)?;
        io::copy(&mut response.into_reader(), &mut out)
            .map_err(|error| format!("could not fetch {}: {error}", pin.url))?;
        out.flush()?;
        out.sync_all()?;
    }
    let found = sha256_of(&part.0)?;
    if found != pin.sha256 {
        return Err(format!(
            "{} has SHA-256 {found}, not the pinned {}: it was not used. \
             If the file was replaced on purpose where it is published, \
             the pin has to be changed after trying the new one",
            pin.url, pin.sha256
        )
        .into());
    }
    // Another build may have put it there meanwhile: either copy will do.
    let _ = fs::rename(&part.0, &path);
    drop(part);
    fs::write(&checked, pin.sha256)?;
    Ok(path)
}

/// The archive, fetched and unpacked into the cache: the folder it was
/// unpacked into. The archive itself is removed once it is unpacked.
pub fn unpacked(pin: &Pinned) -> Result<PathBuf, BuildError> {
    let root = cache_root();
    let folder = root.join(pin.stem());
    let stamp = folder.join(".dirt-sha256");
    if fs::read_to_string(&stamp).is_ok_and(|seen| seen.trim() == pin.sha256) {
        return Ok(folder);
    }
    clear_left_behind(&root);
    let archive = fetched(pin)?;
    let part = Part(ours(&folder));
    let _ = fs::remove_dir_all(&part.0);
    fs::create_dir_all(&part.0)?;
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(File::open(&archive)?));
    tar.unpack(&part.0)
        .map_err(|error| format!("could not unpack {}: {error}", archive.display()))?;
    // An archive holds one folder of its own name, or its files loose.
    let inner = part.0.join(pin.stem());
    let made = if inner.is_dir() {
        inner
    } else {
        part.0.clone()
    };
    fs::write(made.join(".dirt-sha256"), pin.sha256)?;
    if folder.exists() {
        // Left by a build that stopped half way, or made meanwhile by
        // another one: the stamp says which.
        if fs::read_to_string(&stamp).is_ok_and(|seen| seen.trim() == pin.sha256) {
            return Ok(folder);
        }
        fs::remove_dir_all(&folder)?;
    }
    if fs::rename(&made, &folder).is_err() && !stamp.is_file() {
        return Err(format!("could not put {} in place", folder.display()).into());
    }
    drop(part);
    let _ = fs::remove_file(&archive);
    let _ = fs::remove_file(archive.with_file_name(format!("{}.sha256", pin.name)));
    Ok(folder)
}

/// Puts `source` at `target`: a hard link where the two are on one disk,
/// so that nothing is kept twice, and a copy where they are not. A file
/// already there of the same size is taken to be it.
pub fn link_or_copy(source: &Path, target: &Path) -> io::Result<()> {
    if let (Ok(have), Ok(want)) = (fs::metadata(target), fs::metadata(source)) {
        if have.len() == want.len() {
            return Ok(());
        }
        fs::remove_file(target)?;
    }
    if fs::hard_link(source, target).is_ok() {
        return Ok(());
    }
    fs::copy(source, target).map(|_| ())
}
