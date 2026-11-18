# Common settings of trial 2's scripts. Source it.
export W="C:/Users/login/.t3/worktrees/dirt/t3-09936129-ea7938d8d3624b86"
export S="C:/Users/login/AppData/Local/Temp/claude/93483e79a9718c57/a3c71c22-ff5b-4cb8-ad7d-c3aeb1ceac0b/scratchpad/t2"
export DEV="emulator-5554"
export PKG="com.example.DirtMobile"
export PORT=18674
export PHONE_PORT=18675
export SIGN_IN_PORT=18680
export GEN="$W/target/dx/dirt-mobile/debug/android/app"
export MSYS_NO_PATHCONV=1
a() { adb -s "$DEV" "$@"; }
