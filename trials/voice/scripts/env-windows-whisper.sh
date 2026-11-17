# Source this before building the whisper feature on Windows from Git Bash.
# Everything here is set for the calling shell only; nothing global changes.
# Adjust the versions to what the machine has.
#
# 1. bindgen needs libclang (Visual Studio's "C++ Clang tools" has one), and
#    libclang must be told where the C headers are when the shell is not a
#    Visual Studio developer prompt. Without it whisper-rs-sys falls back to
#    bindings generated on Linux, which do not compile for Windows.
vs="C:/Program Files/Microsoft Visual Studio/2022/Preview"
msvc="$vs/VC/Tools/MSVC/14.40.33617"
kit="C:/Program Files (x86)/Windows Kits/10/Include/10.0.22621.0"
export LIBCLANG_PATH="$vs/VC/Tools/Llvm/x64/bin"
export BINDGEN_EXTRA_CLANG_ARGS="-I\"$vs/VC/Tools/Llvm/x64/lib/clang/17/include\" -I\"$msvc/include\" -I\"$kit/ucrt\" -I\"$kit/um\" -I\"$kit/shared\""
#
# 2. With the Visual Studio generator the `cmake` crate replaces CMake's
#    release flags with its own, which carry no optimisation level, so
#    whisper.cpp is compiled unoptimised (/Od) even in a release build:
#    tiny.en took 20 s for one reading instead of a fraction of a second.
#    whisper-rs-sys passes any CMAKE_* variable through, so say it here.
export CMAKE_C_FLAGS_RELEASE="-nologo -MD -O2 -Ob2 -DNDEBUG"
export CMAKE_CXX_FLAGS_RELEASE="-nologo -MD -O2 -Ob2 -DNDEBUG"
