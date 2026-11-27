# Third-party notices

Dirt is under the MIT licence (`LICENSE`). The apps ship these works of
others, each under its own licence. Each licence's text is in the
repository beside the files, and is installed with the app where the
licence asks for it. The release workflow attaches this file and `LICENSE`
to each release.

## In the desktop and Android apps

| What | By | Licence | Where its notice is |
|---|---|---|---|
| Parakeet TDT-CTC 110M, the speech model, as converted to ONNX and quantised to 8 bits by the sherpa-onnx project | NVIDIA, <https://huggingface.co/nvidia/parakeet-tdt_ctc-110m> | Creative Commons Attribution 4.0 International (CC BY 4.0), <https://creativecommons.org/licenses/by/4.0/> | `crates/dirt-voice/NOTICE.txt`, shipped as `NOTICE.txt` in the app's voice folder (the installed app's `assets` folder on Windows; the APK's `assets/voice/`) |
| Silero VAD, which finds the pauses in speech | The Silero Team, <https://github.com/snakers4/silero-vad> | MIT | the same `NOTICE.txt` |
| sherpa-onnx, the speech engine's libraries | The k2-fsa project, <https://github.com/k2-fsa/sherpa-onnx> | Apache 2.0 | the same `NOTICE.txt` |
| ONNX Runtime, built into sherpa-onnx's libraries | Microsoft, <https://github.com/microsoft/onnxruntime> | MIT | the same `NOTICE.txt` |
| Inter 4.1, the interface's typeface | Rasmus Andersson, <https://rsms.me/inter/> | SIL Open Font License 1.1 | `crates/dirt-ui/assets/fonts/Inter-LICENSE.txt` |
| JetBrains Mono 2.304 | JetBrains, <https://www.jetbrains.com/lp/mono/> | SIL Open Font License 1.1 | `crates/dirt-ui/assets/fonts/JetBrainsMono-LICENSE.txt` |
| Lucide's icons, redrawn in `dirt-ui` | Lucide Contributors, <https://lucide.dev> (and Feather's, under MIT) | ISC | `crates/dirt-ui/assets/icons/Lucide-LICENSE.txt` |
| PDF.js 6.4.299, which shows PDFs, with its fonts, character maps and decoders | Mozilla, <https://mozilla.github.io/pdf.js/> | Apache 2.0, and BSD and MIT for parts | `crates/dirt-ui/assets/pdfjs/` (`LICENSE`, `standard_fonts/LICENSE_FOXIT`, `cmaps/LICENSE`, `wasm/LICENSE_*`) |

`crates/dirt-ui/assets/README.md` says which files of each were taken and
why. The model is used as sherpa-onnx's project publishes it, unchanged by
Dirt.

## Rust and Android libraries

The programs are built from Rust crates (`Cargo.lock` lists each with its
version; their licences are MIT, Apache 2.0 or similar, as each crate
declares), and the Android app also from AndroidX and Kotlin libraries
(Apache 2.0) that Gradle brings in.
