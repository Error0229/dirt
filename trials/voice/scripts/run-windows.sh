#!/usr/bin/env bash
# Runs the measurements on this machine: every config three times, taking
# turns, so that a change in the machine's load falls on all of them.
#   bash scripts/run-windows.sh sherpa    (or: whisper, real, pauses)
# The binaries are the ones built with one engine each, copied to data/.
set -u
cd "$(dirname "$0")/.."
W=data/models/whisper
S=data/models/sherpa
out=data/results
mkdir -p "$out" data/pause-results
common="--set data/set/manifest.tsv --high-priority --threads 4"

run() { # binary, name, run number, arguments
  local bin=$1 name=$2 n=$3; shift 3
  [ -s "$out/$name.$n.jsonl" ] && return
  echo "== $name run $n  $(date +%T)"
  "./data/voice-trial-$bin.exe" hear $common --out "$out/$name.$n.jsonl.part" "$@" 2> "data/logs/$name.$n.err" \
    && mv "$out/$name.$n.jsonl.part" "$out/$name.$n.jsonl"
  tail -1 "data/logs/$name.$n.err"
}

case "${1:-}" in
sherpa)
  for n in 1 2 3; do
    run sherpa sherpa-zipformer-20M $n --engine sherpa --model $S/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17 --vad $S/silero_vad.onnx --pretend-clock
    run sherpa sherpa-zipformer-20M-lead $n --engine sherpa --model $S/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17 --lead 0.5 --pretend-clock
    run sherpa sherpa-zipformer-2023-06-26 $n --engine sherpa --model $S/sherpa-onnx-streaming-zipformer-en-2023-06-26 --pretend-clock
    run sherpa sherpa-kroko $n --engine sherpa --model $S/sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06 --pretend-clock
    run sherpa sherpa-nemo-480ms $n --engine sherpa --model $S/sherpa-onnx-nemo-streaming-fast-conformer-transducer-en-480ms-int8 --pretend-clock
    run sherpa second-moonshine-tiny $n --engine sherpa --model $S/sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06 --second-pass $S/sherpa-onnx-moonshine-tiny-en-quantized-2026-02-27 --pretend-clock
    run sherpa second-moonshine-base $n --engine sherpa --model $S/sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06 --second-pass $S/sherpa-onnx-moonshine-base-en-quantized-2026-02-27 --pretend-clock
  done ;;
whisper)
  for n in 1 2 3; do
    run whisper whisper-tiny.en $n --engine whisper --model $W/ggml-tiny.en.bin --pretend-clock
    run whisper whisper-tiny.en-short $n --engine whisper --model $W/ggml-tiny.en.bin --short-window --pretend-clock
    run whisper whisper-base.en-short $n --engine whisper --model $W/ggml-base.en.bin --short-window --pretend-clock
    run whisper whisper-base.en-q5-short $n --engine whisper --model $W/ggml-base.en-q5_1.bin --short-window --pretend-clock
    run whisper whisper-small.en-short $n --engine whisper --model $W/ggml-small.en.bin --short-window --pretend-clock
  done ;;
real)
  # The same on the wall clock, once each, to check the pretend clock.
  run sherpa real-sherpa-kroko 1 --engine sherpa --model $S/sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06 --vad $S/silero_vad.onnx
  run whisper real-whisper-tiny.en-short 1 --engine whisper --model $W/ggml-tiny.en.bin --short-window
  ;;
pauses)
  out=data/pause-results
  common="--set data/pauses/manifest.tsv --high-priority --threads 4"
  run sherpa pauses-kroko 1 --engine sherpa --model $S/sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06 --vad $S/silero_vad.onnx --pretend-clock
  run sherpa pauses-nemo 1 --engine sherpa --model $S/sherpa-onnx-nemo-streaming-fast-conformer-transducer-en-480ms-int8 --pretend-clock
  ./data/voice-trial-whisper.exe whisper-vad --model $W/ggml-silero-v5.1.2.bin --set data/pauses/manifest.tsv --out $out/pauses-whisper-vad.jsonl 2> data/logs/pauses-whisper-vad.err
  ;;
*) echo "which: sherpa, whisper, real or pauses"; exit 2 ;;
esac
