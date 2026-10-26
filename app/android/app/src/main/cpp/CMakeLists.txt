# The native audio half: libopus for compression, and the few lines of JNI that let Kotlin reach it.
#
# There was a noise suppressor here too -- RNNoise, built for Android over four separate build
# faults. It was removed on 2026-10-26, the day after it was first heard: it is a 2017 model built
# for STEADY noise and the verdict was that a keyboard and mouse still came through it. Cleaning
# moved to the PC, where there is the processor for a model that handles transients, and the phone
# now simply asks for it with a flag. See protocol/README.md.
#
# Why the real library rather than Android's built-in MediaCodec Opus encoder:
#
#   - MediaCodec's Opus encoder expects 48 kHz. Earshot's capture rate is not always 48 kHz: the
#     voice-call source on many phones only offers 16 kHz, and libopus takes that directly while
#     MediaCodec would need us to resample first.
#   - It gives no control over in-band forward error correction. The receiver rebuilds a lost frame
#     from the redundant copy in the next packet, and only the encoder can decide to put one there.
#   - It is a vendor codec, so its behaviour varies by phone. libopus behaves the same everywhere,
#     and is the same library the receiver decodes with.
#
# The source is downloaded at configure time from a pinned URL and checked against a pinned hash,
# rather than vendored into the repository: 4 MB of C that nobody here reads is not worth carrying
# in git, and a hash is a stronger statement than a copy anyway.
#
# ⚠️ This step needs network access the first time a machine configures the build. It is cached in
# the build directory afterwards.

cmake_minimum_required(VERSION 3.18)
project(earshot_audio C)

include(FetchContent)

# Where the fetched sources live.
#
# ⚠️ Android builds this directory **once per processor** — arm64, arm32, x86_64 — and by default
# each of those gets its own `_deps`, so a clean build downloads Opus three times. On a slow link
# that is most of the build.
#
# Pointing the cache outside the build tree makes it survive `flutter clean` as well, so the only
# machine that ever waits for these downloads is one that has never built Earshot. Still one cache
# per processor: two configures running at once must not share a download directory.
if(DEFINED ENV{HOME})
    set(FETCHCONTENT_BASE_DIR "$ENV{HOME}/.cache/earshot-native/${ANDROID_ABI}" CACHE PATH "" FORCE)
endif()

set(OPUS_VERSION 1.5.2)

# Opus's own CMake options. Everything Earshot does not use is off, which keeps the library small:
# no encoder extras we do not reach, no docs, no tests, no command-line tools.
set(OPUS_BUILD_SHARED_LIBRARY OFF CACHE BOOL "" FORCE)
set(OPUS_BUILD_TESTING OFF CACHE BOOL "" FORCE)
set(OPUS_BUILD_PROGRAMS OFF CACHE BOOL "" FORCE)
set(OPUS_INSTALL_PKG_CONFIG_MODULE OFF CACHE BOOL "" FORCE)
set(OPUS_INSTALL_CMAKE_CONFIG_MODULE OFF CACHE BOOL "" FORCE)

FetchContent_Declare(
    opus
    URL https://downloads.xiph.org/releases/opus/opus-${OPUS_VERSION}.tar.gz
    URL_HASH SHA256=65c1d2f78b9f2fb20082c38cbe47c951ad5839345876e46941612ee87f9a7ce1
)
FetchContent_MakeAvailable(opus)

add_library(earshot_audio SHARED opus_jni.c)

target_link_libraries(earshot_audio PRIVATE opus log)

# -O2: this runs once per 20 ms frame on the audio path.
target_compile_options(earshot_audio PRIVATE -O2 -Wall -Wextra)
