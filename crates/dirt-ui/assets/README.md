# What is bundled, and under which licence

Nothing here is fetched at run time (interface design, section 4.2).

| What | From | Licence |
|---|---|---|
| `fonts/InterVariable.woff2` | Inter 4.1, `web/InterVariable.woff2` of the release at <https://github.com/rsms/inter/releases/tag/v4.1>. The variable font, upright only: the design uses weights between the named ones | SIL Open Font License 1.1, `fonts/Inter-LICENSE.txt` |
| `fonts/JetBrainsMono-Medium.woff2` | JetBrains Mono 2.304, `fonts/webfonts/JetBrainsMono-Medium.woff2` of the release at <https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304>. One weight, 500, which is the only one the design uses | SIL Open Font License 1.1, `fonts/JetBrainsMono-LICENSE.txt` |
| The icons, which are drawn in `src/icons.rs` and have no files here | Lucide, <https://lucide.dev>, by way of the design's mockup: Lucide's drawings with their numbers rounded, and a few simplified or redrawn in the same style | ISC, and MIT for the icons Lucide took from Feather, `icons/Lucide-LICENSE.txt` |
| `pdfjs/pdf.min.mjs`, `pdfjs/pdf.worker.min.mjs` | PDF.js 6.4.299, `legacy/build/` of the npm package `pdfjs-dist@6.4.299` (published 2026-10-03), as released. The build for older browsers, so that an older WebKitGTK or Android web view draws PDFs too. 0.50 MiB and 1.26 MiB | Apache 2.0, `pdfjs/LICENSE` |
| `pdfjs/standard_fonts/` | The same package's `standard_fonts/`: the Foxit fonts PDF.js draws a PDF's standard fonts with when the PDF does not carry them. The Liberation Sans files beside them in the package are left out, as they are under the GPL: Helvetica and Arial not carried by a PDF are drawn in the system's sans-serif instead. 0.19 MiB | BSD-style, `pdfjs/standard_fonts/LICENSE_FOXIT` |
| `pdfjs/cmaps/` | The same package's `cmaps/`: Adobe's character maps, for CJK text in fonts a PDF does not carry. 1.11 MiB | BSD 3-clause, `pdfjs/cmaps/LICENSE` |
| `pdfjs/wasm/` | The same package's decoders for JPEG 2000 and JBIG2 images and its colour management (`openjpeg.wasm`, `jbig2.wasm`, `qcms_bg.wasm`). Its JavaScript stand-ins for a web view without WebAssembly, and its QuickJS for running a PDF's scripts, are left out: every web view the apps run in has WebAssembly, and a PDF's scripts are never run. 0.45 MiB | BSD 2-clause, BSD 3-clause, Apache 2.0 and MIT, `pdfjs/wasm/LICENSE_*` |

The font files are as released, not subsetted. A new font or icon set is
added with its licence beside it and a row here.

PDF.js is 3.5 MiB in 192 files in all. It is in `dirt-ui`, which only the
two apps build: the CLI and the server carry none of it. The apps load it
only when a PDF is first shown (`src/files/pdf.rs`). To move to a newer
release, replace these files with the same ones of that release, keep the
same files out, and change the version and the sizes here and in the
interface design's section 11.
