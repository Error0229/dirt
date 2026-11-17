#!/usr/bin/env bash
# The phone's half of the trial. NOT RUN YET: it needs the owner's word
# first, and it was written without touching any phone or the emulator.
#
# It pushes the measuring program, the libraries it loads, one streaming
# model, one whisper model and the test set to the phone's
# /data/local/tmp/voice-trial, runs each engine three times at the speed of
# speech, brings the result files back, and removes everything it pushed.
# Nothing is installed, and nothing but that folder is touched.
#
# Before: build the program (bash scripts/build-android.sh
# aarch64-linux-android "whisper sherpa"), fetch the models and make the
# test set, connect one phone with USB debugging on, and check that
# `adb devices` lists it and only it. Afterwards:
#   python -I scripts/analyse.py data/phone-results
set -eu
cd "$(dirname "$0")/.."
export MSYS_NO_PATHCONV=1   # Git Bash must not rewrite /data/local/tmp
sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:?set ANDROID_SDK_ROOT or ANDROID_HOME}}"
ndk_libs="$sdk/ndk/26.3.11579264/toolchains/llvm/prebuilt/windows-x86_64/sysroot/usr/lib/aarch64-linux-android"
there=/data/local/tmp/voice-trial
S=data/models/sherpa
streaming=sherpa-onnx-streaming-zipformer-en-2023-06-26
second=sherpa-onnx-moonshine-tiny-en-quantized-2026-02-27

adb devices
adb shell "mkdir -p $there/set $there/models/$streaming $there/models/$second"
adb push target/aarch64-linux-android/release/voice-trial "$there/"
adb push target/sherpa-onnx-prebuilt/jniLibs/arm64-v8a/libsherpa-onnx-c-api.so "$there/"
adb push target/sherpa-onnx-prebuilt/jniLibs/arm64-v8a/libonnxruntime.so "$there/"
adb push "$ndk_libs/libc++_shared.so" "$there/"
adb push data/set/manifest.tsv "$there/set/"
for wav in data/set/*.wav; do adb push "$wav" "$there/set/"; done
for f in "$S/$streaming"/*.int8.onnx "$S/$streaming/tokens.txt"; do adb push "$f" "$there/models/$streaming/"; done
for f in "$S/$second"/*.ort "$S/$second/tokens.txt"; do adb push "$f" "$there/models/$second/"; done
adb push "$S/silero_vad.onnx" "$there/models/"
adb push data/models/whisper/ggml-base.en-q5_1.bin "$there/models/"
adb push data/models/whisper/ggml-tiny.en.bin "$there/models/"
adb shell "chmod 755 $there/voice-trial"

run() { # name, arguments
  local name=$1; shift
  for n in 1 2 3; do
    adb shell "cd $there && LD_LIBRARY_PATH=. ./voice-trial hear --set set/manifest.tsv --threads 4 --out $name.$n.jsonl $*"
  done
}
run phone-sherpa-zipformer --engine sherpa --model "models/$streaming" --vad models/silero_vad.onnx
run phone-second-moonshine-tiny --engine sherpa --model "models/$streaming" --second-pass "models/$second"
run phone-whisper-tiny.en-short --engine whisper --model models/ggml-tiny.en.bin --short-window
run phone-whisper-base.en-q5-short --engine whisper --model models/ggml-base.en-q5_1.bin --short-window

mkdir -p data/phone-results
for f in $(adb shell "ls $there/*.jsonl" | tr -d '\r'); do adb pull "$f" data/phone-results/; done
adb shell "rm -r $there"
echo "results are in data/phone-results"
