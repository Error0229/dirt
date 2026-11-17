#!/usr/bin/env bash
# Fetches the models the trial measures into trials/voice/data/models.
# Nothing here is committed. Run from anywhere: paths are relative to this file.
set -u
here="$(cd "$(dirname "$0")/.." && pwd)"
out="$here/data/models"
mkdir -p "$out/whisper" "$out/sherpa"

fetch() { # url, file
  if [ -s "$2" ]; then echo "have $(basename "$2")"; return; fi
  echo "get  $(basename "$2")"
  curl -sSL --retry 5 --retry-delay 5 -A "voice-trial" -o "$2.part" "$1" && mv "$2.part" "$2"
}

# whisper.cpp's ggml models (MIT), published by the whisper.cpp project.
hf="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"
for m in tiny.en base.en small.en base.en-q5_1; do
  fetch "$hf/ggml-$m.bin" "$out/whisper/ggml-$m.bin"
done
# Silero VAD in ggml form, for whisper.cpp's built-in VAD.
fetch "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin" "$out/whisper/ggml-silero-v5.1.2.bin"

# sherpa-onnx's models, published on the project's "asr-models" release.
gh="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models"
for m in \
  sherpa-onnx-streaming-zipformer-en-20M-2023-02-17 \
  sherpa-onnx-streaming-zipformer-en-2023-06-26 \
  sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06 \
  sherpa-onnx-nemo-streaming-fast-conformer-transducer-en-480ms-int8 \
  sherpa-onnx-moonshine-tiny-en-quantized-2026-02-27 \
  sherpa-onnx-moonshine-base-en-quantized-2026-02-27; do
  if [ -d "$out/sherpa/$m" ]; then echo "have $m"; continue; fi
  fetch "$gh/$m.tar.bz2" "$out/sherpa/$m.tar.bz2"
  tar -xjf "$out/sherpa/$m.tar.bz2" -C "$out/sherpa" && rm "$out/sherpa/$m.tar.bz2"
done
fetch "$gh/silero_vad.onnx" "$out/sherpa/silero_vad.onnx"
echo done
