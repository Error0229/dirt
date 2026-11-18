#!/usr/bin/env bash
# Builds the Android app for the emulator, strips the library, packages it.
# Usage: build-android.sh [extra env assignments are taken from the caller]
. "$(dirname "$0")/env.sh"
set -e
cd "$W/crates/dirt-mobile"
start=$(date +%s)
JAVA_HOME="C:/Users/login/scoop/apps/openjdk21/current" \
CARGO_BUILD_JOBS=4 \
DIRT_MOBILE_API_BASE_URL="http://127.0.0.1:$PHONE_PORT" \
  dx build --platform android < /dev/null > "$S/dx-build.log" 2>&1 || { echo "dx failed"; tail -40 "$S/dx-build.log"; exit 1; }
echo "dx build took $(( $(date +%s) - start ))s"
LIB="$GEN/app/src/main/jniLibs/x86_64/libmain.so"
ls -l "$LIB" | awk '{print "unstripped bytes:", $5}'
"$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-strip.exe" --strip-unneeded "$LIB"
ls -l "$LIB" | awk '{print "stripped bytes:", $5}'
cd "$GEN"
JAVA_HOME="C:/Users/login/scoop/apps/openjdk21/current" ./gradlew.bat assembleDebug < /dev/null > "$S/gradle.log" 2>&1 || { echo "gradle failed"; tail -40 "$S/gradle.log"; exit 1; }
ls -l "$GEN/app/build/outputs/apk/debug/app-debug.apk" | awk '{print "apk bytes:", $5}'
echo "all took $(( $(date +%s) - start ))s"
