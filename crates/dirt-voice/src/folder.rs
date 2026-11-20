//! The voice folder: the model, Silero's detector and, on Windows, the
//! engine's libraries. `build.rs` puts it together from the build cache;
//! an app ships it and says where it is at run time.

use std::path::{Path, PathBuf};

use crate::{Result, VoiceError};

/// The detector's file.
pub const DETECTOR: &str = "silero_vad.onnx";

/// The model's files.
pub const MODEL_FILES: [&str; 4] = [
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joiner.int8.onnx",
    "tokens.txt",
];

/// A voice folder that has at least the detector in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceFolder {
    path: PathBuf,
}

impl VoiceFolder {
    /// The folder at `path`, if the detector is in it.
    pub fn at(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if !path.join(DETECTOR).is_file() {
            return Err(VoiceError::NotInstalled(format!(
                "{} has no {DETECTOR}",
                path.display()
            )));
        }
        Ok(Self { path })
    }

    /// The folder this crate was built with, on the computer that built
    /// it: for its tests, and for a development build run where it was
    /// built. A program that is shipped names its own.
    #[must_use]
    pub fn built() -> Option<Self> {
        option_env!("DIRT_VOICE_BUILT_FOLDER")
            .filter(|path| !path.is_empty())
            .and_then(|path| Self::at(path).ok())
    }

    /// Where it is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the model is in it. A build made with
    /// `DIRT_VOICE_MODEL=skip` has the detector alone: it can listen and
    /// keep a recording, and not turn it into words.
    #[must_use]
    pub fn has_model(&self) -> bool {
        MODEL_FILES
            .iter()
            .all(|file| self.path.join(file).is_file())
    }

    pub(crate) fn file(&self, name: &str) -> String {
        self.path.join(name).to_string_lossy().into_owned()
    }

    /// Makes the engine's code ready to call. On Windows its libraries
    /// are loaded from this folder, once for the program: the first
    /// folder asked for is the one used. Elsewhere they are linked in, or
    /// found by the system (Android), and this only checks that the folder
    /// is still there.
    pub(crate) fn load_engine(&self) -> Result<()> {
        #[cfg(windows)]
        {
            windows::load(&self.path)
        }
        #[cfg(not(windows))]
        {
            if self.path.join(DETECTOR).is_file() {
                Ok(())
            } else {
                Err(VoiceError::NotInstalled(format!(
                    "{} has gone",
                    self.path.display()
                )))
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use windows_sys::Win32::System::LibraryLoader::{
        LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH,
    };

    use crate::{Result, VoiceError};

    /// ONNX Runtime first: Windows 11 has a library of the same name of its
    /// own, of another version, and the engine must have the one beside
    /// it. Then the engine, which the program calls: the linker made those
    /// calls look for it only when first made, and they find it loaded.
    const LIBRARIES: [&str; 2] = ["onnxruntime.dll", "sherpa-onnx-c-api.dll"];

    /// Loaded once, for the program. A load that failed is not
    /// remembered: the next listening or reading tries again, so a file put
    /// back, or a microphone plugged in later, needs no restart.
    pub fn load(folder: &Path) -> Result<()> {
        static LOADED: AtomicBool = AtomicBool::new(false);
        static LOADING: Mutex<()> = Mutex::new(());
        if LOADED.load(Ordering::SeqCst) {
            return Ok(());
        }
        let _one_at_a_time = LOADING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if LOADED.load(Ordering::SeqCst) {
            return Ok(());
        }
        let loaded = (|| {
            for name in LIBRARIES {
                let path = folder.join(name);
                let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
                // SAFETY: a path ending in a nul, and no handle to the
                // file asked for. The library stays loaded for as long
                // as the program runs, which is what is wanted.
                #[allow(unsafe_code)]
                let module = unsafe {
                    LoadLibraryExW(
                        wide.as_ptr(),
                        std::ptr::null_mut(),
                        LOAD_WITH_ALTERED_SEARCH_PATH,
                    )
                };
                if module.is_null() {
                    return Err(format!(
                        "{} could not be loaded ({})",
                        path.display(),
                        std::io::Error::last_os_error()
                    ));
                }
            }
            Ok(())
        })();
        if loaded.is_ok() {
            LOADED.store(true, Ordering::SeqCst);
        }
        loaded.map_err(VoiceError::NotInstalled)
    }
}
