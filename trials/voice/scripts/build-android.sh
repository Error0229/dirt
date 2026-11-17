#!/usr/bin/env bash
# Cross-compiles the measuring program for Android with NDK 26.3, API 24.
#   bash scripts/build-android.sh aarch64-linux-android sherpa
#   bash scripts/build-android.sh x86_64-linux-android whisper
# Everything is set for this script only; nothing global changes.
set -eu
cd "$(dirname "$0")/.."
target=$1
features=${2:-}
sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:?set ANDROID_SDK_ROOT or ANDROID_HOME}}"
ndk="$(cygpath -m "$sdk")/ndk/26.3.11579264"
bin="$ndk/toolchains/llvm/prebuilt/windows-x86_64/bin"
api=24
case "$target" in
  aarch64-linux-android) abi=arm64-v8a ;;
  x86_64-linux-android)  abi=x86_64 ;;
  *) echo "unknown target $target"; exit 2 ;;
esac
under=${target//-/_}
upper=$(echo "$under" | tr a-z A-Z)

# For cargo and for the `cc` crate.
export "CARGO_TARGET_${upper}_LINKER=$bin/${target}${api}-clang.cmd"
export "CC_${under}=$bin/${target}${api}-clang.cmd"
export "CXX_${under}=$bin/${target}${api}-clang++.cmd"
export "AR_${under}=$bin/llvm-ar.exe"

# For whisper.cpp's CMake build: the NDK's own toolchain file (through a
# wrapper that tells it the ABI), with Ninja. whisper-rs-sys hands every
# CMAKE_* variable to CMake.
export TRIAL_ANDROID_ABI=$abi TRIAL_ANDROID_API=$api TRIAL_ANDROID_NDK="$ndk"
export CMAKE_TOOLCHAIN_FILE="$(cygpath -m "$PWD")/scripts/android.toolchain.cmake"
export CMAKE_GENERATOR=Ninja
# Naming the flags ourselves keeps two wrong ones out: whisper-rs-sys adds
# MSVC's "/utf-8" whenever the computer doing the build runs Windows, which
# clang takes for a file, and the `cmake` crate adds a --target of its own.
# -O3 because naming the flags also replaces CMake's own for a release
# build. The page size is for phones with 16 KB memory pages.
export CMAKE_C_FLAGS="-fPIC -O3 -DNDEBUG"
export CMAKE_CXX_FLAGS="-fPIC -O3 -DNDEBUG"
export CMAKE_SHARED_LINKER_FLAGS="-Wl,-z,max-page-size=16384"

# For bindgen: a libclang (Visual Studio's serves), its own headers, and
# the NDK's.
vs_llvm="${VS_LLVM:-C:/Program Files/Microsoft Visual Studio/2022/Preview/VC/Tools/Llvm/x64}"
export LIBCLANG_PATH="${LIBCLANG_PATH:-$vs_llvm/bin}"
export "BINDGEN_EXTRA_CLANG_ARGS_${under}=--sysroot=\"$ndk/toolchains/llvm/prebuilt/windows-x86_64/sysroot\" -I\"$vs_llvm/lib/clang/17/include\""

# whisper-rs-sys 0.15 asks the linker for advapi32 whenever the computer
# doing the build runs Windows, whatever the target. An empty library of
# that name lets the link go through; the real fix is a patch to the crate.
stubs="$(cygpath -m "$PWD")/target/android-stubs"
mkdir -p "$stubs"
[ -f "$stubs/libadvapi32.a" ] || "$bin/llvm-ar.exe" rc "$stubs/libadvapi32.a"
export "CARGO_TARGET_${upper}_RUSTFLAGS=-L $stubs"

start=$(date +%s)
cargo build --release --target "$target" ${features:+--features "$features"}
echo "built $target ${features:-with no engine} in $(( $(date +%s) - start )) s"
ls -la "target/$target/release/voice-trial"
