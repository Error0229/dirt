# The NDK's toolchain file needs the ABI and the API level as CMake
# variables, and whisper-rs-sys can only hand CMake variables whose names
# start with CMAKE_, WHISPER_ or GGML_. So scripts/build-android.sh names
# this file as the toolchain, and it reads the rest from the environment.
set(ANDROID_ABI "$ENV{TRIAL_ANDROID_ABI}")
set(ANDROID_PLATFORM "android-$ENV{TRIAL_ANDROID_API}")
include("$ENV{TRIAL_ANDROID_NDK}/build/cmake/android.toolchain.cmake")
