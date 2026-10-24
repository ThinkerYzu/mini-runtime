#!/bin/sh
# Builds realsite for a phone (aarch64 Android, task 6da8): target/aarch64-linux-android/release/realsite,
# which runs next to obj-js-android's libmozjs. Needs SpiderMonkey built for Android first
# (a SpiderMonkey Android build) and the NDK mach bootstraps (r29).
set -e
cd "$(dirname "$0")/.."
NDK=${NDK:-$HOME/.mozbuild/android-ndk-r29}
API=${API:-34}
BIN=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin
TREE=${FIREFOX_SRC:?Set FIREFOX_SRC to your Firefox source checkout}
export MRT_JS_OBJDIR=${MRT_JS_OBJDIR:-$TREE/obj-js-android}
export CXX=$BIN/aarch64-linux-android$API-clang++
export CC=$BIN/aarch64-linux-android$API-clang
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$CC
export AR=$BIN/llvm-ar

# brotli from the tree (Firefox's own copy), as static archives; the host links the system's
BROTLI=$PWD/target/aarch64-linux-android/brotli
if [ ! -f $BROTLI/libbrotlienc.a ]; then
  mkdir -p $BROTLI/obj
  for part in common dec enc; do
    rm -f $BROTLI/obj/*.o
    for f in $TREE/modules/brotli/$part/*.c; do
      $CC -O2 -fPIC -I$TREE/modules/brotli/include -c $f -o $BROTLI/obj/$part-$(basename $f .c).o
    done
    $AR crs $BROTLI/libbrotli$part.a $BROTLI/obj/*.o
  done
fi
export MRT_BROTLI_LIB=$BROTLI
# only the NDK's static libc++ in its own directory: its sysroot directory also has libc.a, which -lc
# would then find before the shared libc
CXXLIB=$PWD/target/aarch64-linux-android/cxx
mkdir -p $CXXLIB
cp $NDK/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_static.a \
   $NDK/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++abi.a $CXXLIB/
export MRT_CXX_LIBDIR=$CXXLIB
cargo build --release --target aarch64-linux-android --bin realsite "$@"
