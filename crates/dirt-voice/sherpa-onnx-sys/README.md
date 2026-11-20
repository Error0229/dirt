# sherpa-onnx-sys, as Dirt builds it

`src/` and `LICENSE` are sherpa-onnx's own, from version 2.0.1 of the
`sherpa-onnx-sys` crate on crates.io (Apache-2.0, the k2-fsa project), and
are not changed. Only `build.rs` and `Cargo.toml` are Dirt's.

The published build script downloads the project's prebuilt libraries into
`target/` and does not check them. This one fetches the same archives from
the same release, checks each against the SHA-256 written in `build.rs`, and
keeps them in a cache outside `target/` (`../build/fetch.rs`), so that a
`cargo clean` does not fetch them again (implementation plan, trial 1).

How it links, by target:

| Target | Libraries | Why |
|---|---|---|
| Windows x64 | Shared: `sherpa-onnx-c-api.dll` and `onnxruntime.dll`, shipped in the voice folder beside the model, loaded late | The static ones need the C runtime linked statically and a newer MSVC than 14.40 (trial 1) |
| Android | Shared, for each ABI | The project publishes no other |
| Linux, macOS | Static | Nothing has to be found beside the program when it starts |

The root `Cargo.toml` puts this crate in place of crates.io's with
`[patch.crates-io]`. To move to another version of sherpa-onnx: take `src/`
from that version of the crate, change the version here and in
`dirt-voice`, and change the archives' names and hashes in `build.rs`. A
release's hashes are published with it: `gh api
repos/k2-fsa/sherpa-onnx/releases/tags/vX.Y.Z --jq '.assets[] | [.name,
.digest]'`.
