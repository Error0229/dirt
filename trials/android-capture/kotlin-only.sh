#!/usr/bin/env bash
# Kotlin, resources or manifest changed and Rust did not: copy what dx
# copies, and package again with Gradle alone.
. "$(dirname "$0")/env.sh"
set -e
cp -r "$W/crates/dirt-mobile/android/native/." "$GEN/plugins/dirtnative/"
cp "$W/crates/dirt-mobile/android/AndroidManifest.xml" "$GEN/app/src/main/AndroidManifest.xml"
cp "$W/crates/dirt-mobile/android/MainActivity.kt" "$GEN/app/src/main/kotlin/dev/dioxus/main/MainActivity.kt"
cd "$GEN"
JAVA_HOME="C:/Users/login/scoop/apps/openjdk21/current" ./gradlew.bat assembleDebug < /dev/null > "$S/gradle.log" 2>&1 || { echo "gradle failed"; grep -E "^e: |error:|FAILED|What went wrong" -A6 "$S/gradle.log" | head -40; exit 1; }
tail -3 "$S/gradle.log"
bash "$S/install.sh"
