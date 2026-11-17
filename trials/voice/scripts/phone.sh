#!/usr/bin/env bash
# The phone's half of the trial. NOT RUN YET: it needs the owner's word
# first, and it was written without touching any phone or the emulator.
#
#   bash scripts/phone.sh hear              the first measurement: words while listening
#   bash scripts/phone.sh read NAME...      the second: reading a finished recording,
#                                           NAME as in `bash scripts/run2.sh list`
#
# Everything goes to the phone's /data/local/tmp/voice-trial, results come
# back to data/phone-results (hear) or data/phone-results2 (read), and the
# folder is removed at the end. Nothing is installed, and nothing but that
# folder is touched.
#
# Before: build the program for the phone
#   bash scripts/build-android.sh aarch64-linux-android "whisper sherpa"
# make the test sets (README), connect one phone with USB debugging on, and
# check that `adb devices` lists it and only it. Afterwards:
#   python -I scripts/analyse.py data/phone-results                    (hear)
#   RESULTS=data/phone-results2 data/venv/Scripts/python scripts/analyse2.py   (read)
#
# `read` measures, for every model named: the 5, 15 and 60 second clips three
# times each (time and memory), the whole test set once (mistakes), and the
# ten-minute recording three times. Each model is fetched here, pushed,
# measured and removed from the phone before the next, so the phone never
# holds more than one model. Keep the phone on a charger, the screen on and
# nothing else running; write down its model, Android version and how warm
# it was.
set -eu
cd "$(dirname "$0")/.."
export MSYS_NO_PATHCONV=1   # Git Bash must not rewrite /data/local/tmp
sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:?set ANDROID_SDK_ROOT or ANDROID_HOME}}"
ndk_libs="$sdk/ndk/26.3.11579264/toolchains/llvm/prebuilt/windows-x86_64/sysroot/usr/lib/aarch64-linux-android"
there=/data/local/tmp/voice-trial
what=${1:?hear or read}
shift

push_program() {
  adb devices
  adb shell "mkdir -p $there/models"
  adb push target/aarch64-linux-android/release/voice-trial "$there/"
  adb push target/sherpa-onnx-prebuilt/jniLibs/arm64-v8a/libsherpa-onnx-c-api.so "$there/"
  adb push target/sherpa-onnx-prebuilt/jniLibs/arm64-v8a/libonnxruntime.so "$there/"
  adb push "$ndk_libs/libc++_shared.so" "$there/"
  adb shell "chmod 755 $there/voice-trial"
}

push_set() { # local folder, name on the phone
  adb shell "mkdir -p $there/$2"
  adb push "$1/manifest.tsv" "$there/$2/"
  for wav in "$1"/*.wav; do adb push "$wav" "$there/$2/"; done
}

if [ "$what" = "hear" ]; then
  S=data/models/sherpa
  streaming=sherpa-onnx-streaming-zipformer-en-2023-06-26
  second=sherpa-onnx-moonshine-tiny-en-quantized-2026-02-27
  push_program
  push_set data/set set
  adb shell "mkdir -p $there/models/$streaming $there/models/$second"
  for f in "$S/$streaming"/*.int8.onnx "$S/$streaming/tokens.txt"; do adb push "$f" "$there/models/$streaming/"; done
  for f in "$S/$second"/*.ort "$S/$second/tokens.txt"; do adb push "$f" "$there/models/$second/"; done
  adb push "$S/silero_vad.onnx" "$there/models/"
  adb push data/models/whisper/ggml-base.en-q5_1.bin "$there/models/"
  adb push data/models/whisper/ggml-tiny.en.bin "$there/models/"

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
  back=data/phone-results

elif [ "$what" = "read" ]; then
  [ $# -gt 0 ] || { echo "name the models: bash scripts/run2.sh list"; exit 2; }
  back=data/phone-results2
  mkdir -p "$back"
  push_program
  push_set data/timing timing
  push_set data/set2 set2
  push_set data/long long
  for name in "$@"; do
    # run2.sh fetches Silero's model with the first one.
    line=$(MODE=fetch KEEP=1 bash scripts/run2.sh "$name" | grep "^fetched|") || { echo "could not fetch $name"; continue; }
    adb shell "ls $there/models/silero_vad.onnx" >/dev/null 2>&1 || adb push data/models2/silero_vad.onnx "$there/models/"
    IFS='|' read -r _ _ args window <<< "$line"
    local_model=$(echo "$args" | sed -E 's/.*--model ([^ ]+).*/\1/')
    adb push "$local_model" "$there/models/"
    args=${args//data\/models2\//models/}
    cut=""
    [ "$window" != "0" ] && cut="--vad models/silero_vad.onnx --window $window"
    go() { adb shell "cd $there && LD_LIBRARY_PATH=. ./voice-trial read $args $cut --threads 4 $*"; }
    go --set timing/manifest.tsv --repeat 3 --out "$name.timing.jsonl"
    go --set set2/manifest.tsv --out "$name.set.jsonl"
    go --set long/manifest.tsv --repeat 3 --out "$name.long.jsonl"
    for f in "$name.timing.jsonl" "$name.set.jsonl" "$name.long.jsonl"; do adb pull "$there/$f" "$back/"; done
    adb shell "rm -r $there/models/$(basename "$local_model")"
    adb shell "cat /sys/class/thermal/thermal_zone*/temp 2>/dev/null | head -3" > "$back/$name.thermal.txt" || true
  done

else
  echo "hear or read"; exit 2
fi

for f in $(adb shell "ls $there/*.jsonl" | tr -d '\r'); do adb pull "$f" "$back/"; done
adb shell "rm -r $there"
echo "results are in $back"
