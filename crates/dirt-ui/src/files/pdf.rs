//! The PDF renderer bundled with the apps (interface design, sections 6.16
//! and 11; decided 2026-11-18): Mozilla's PDF.js, so that a PDF shows its
//! pages in the preview on Windows, Linux and Android alike.
//!
//! It is PDF.js's own build for older browsers (`legacy/build` of the
//! `pdfjs-dist` package), so that an older `WebKitGTK` or Android web view
//! draws pages too, with the fonts a PDF may name without carrying them,
//! the character maps for CJK text in such fonts, and the decoders for
//! JPEG 2000 and JBIG2 images, all in `assets/pdfjs`. Nothing is fetched
//! at run time. The script loads the renderer only when a PDF is first
//! shown, so it costs nothing until then. It runs with PDF.js's scripting
//! and `eval` both off: a PDF's own scripts never run.
//!
//! It is in `dirt-ui`, which only the two apps build: the CLI and the
//! server carry none of it. Its version, its size and its licence are in
//! `assets/README.md`.

use dioxus::prelude::*;

/// The folder of the renderer, as the web view reaches it. The folder is
/// copied as it is, not processed: PDF.js loads its worker, fonts, maps
/// and decoders from beside itself.
pub const FOLDER: Asset = asset!("/assets/pdfjs");
