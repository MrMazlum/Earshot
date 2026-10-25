# The native audio half: libopus for compression, RNNoise for noise suppression, and the few lines
# of JNI that let Kotlin reach each of them.
#
# Why the real library rather than Android's built-in MediaCodec Opus encoder:
#
#   - MediaCodec's Opus encoder expects 48 kHz. Earshot's capture rate is not always 48 kHz: the
#     noise-cancelled voice source on many phones only offers 16 kHz, and libopus takes that
#     directly while MediaCodec would need us to resample first.
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

# RNNoise — noise suppression, from the same people as Opus and under the same BSD licence.
#
# This is a different job from compression and from Android's own noise suppressor. The phone's
# voice-call chain cleans the signal but often clamps capture to 16 kHz, which costs the top of a
# voice; RNNoise runs on the full-band signal we already have, so it is clean *and* 48 kHz instead
# of one or the other.
#
# Two downloads rather than one: the library ships without its trained weights, and
# `download_model.sh` fetches them separately. Both are pinned by URL and hash here, which is the
# same deal as libopus above — nothing is vendored, and nothing is taken on trust.
#
# ⚠️ RNNoise is 48 kHz only, in 10 ms frames. The Kotlin side knows that and simply does not use it
# at any other rate.
set(RNNOISE_VERSION 0.2)
set(RNNOISE_MODEL 0b50c45)

# ⚠️ Pinned by commit, not by tarball hash. A forge's "download this tag as .tar.gz" archive is
# generated on request and is **not** byte-stable, so a hash pinned against one is a build that
# breaks on somebody else's machine for no reason anyone can see. A commit id is the stronger pin
# anyway: it names the exact tree, and a moved tag cannot change it.
FetchContent_Declare(
    rnnoise
    GIT_REPOSITORY https://gitlab.xiph.org/xiph/rnnoise.git
    GIT_TAG 904a876dce1f9ab8860c0a5000ed151f9f6eef58  # v${RNNOISE_VERSION}
    GIT_SHALLOW FALSE
)
FetchContent_MakeAvailable(rnnoise)

# The weights, unpacked straight into the library's own src/ where its build expects them.
FetchContent_Declare(
    rnnoise_model
    URL https://media.xiph.org/rnnoise/models/rnnoise_data-${RNNOISE_MODEL}.tar.gz
    URL_HASH SHA256=4ac81c5c0884ec4bd5907026aaae16209b7b76cd9d7f71af582094a2f98f4b43
    SOURCE_DIR ${rnnoise_SOURCE_DIR}
    DOWNLOAD_EXTRACT_TIMESTAMP TRUE
)
FetchContent_MakeAvailable(rnnoise_model)

# Built by hand rather than through RNNoise's own build: it ships autotools, which is not something
# the Android toolchain drives. The file list is RNNOISE_SOURCES from its Makefile.am.
add_library(rnnoise STATIC
    ${rnnoise_SOURCE_DIR}/src/denoise.c
    ${rnnoise_SOURCE_DIR}/src/rnn.c
    ${rnnoise_SOURCE_DIR}/src/pitch.c
    ${rnnoise_SOURCE_DIR}/src/kiss_fft.c
    ${rnnoise_SOURCE_DIR}/src/celt_lpc.c
    ${rnnoise_SOURCE_DIR}/src/nnet.c
    ${rnnoise_SOURCE_DIR}/src/nnet_default.c
    ${rnnoise_SOURCE_DIR}/src/parse_lpcnet_weights.c
    ${rnnoise_SOURCE_DIR}/src/rnnoise_data.c
    ${rnnoise_SOURCE_DIR}/src/rnnoise_tables.c
)
target_include_directories(rnnoise PUBLIC
    ${rnnoise_SOURCE_DIR}/include
    ${rnnoise_SOURCE_DIR}/src
)
target_compile_options(rnnoise PRIVATE -O3 -Wno-unused-function)

add_library(earshot_audio SHARED opus_jni.c rnnoise_jni.c)

target_link_libraries(earshot_audio PRIVATE opus rnnoise log)

# -O2: this runs once per 20 ms frame on the audio path.
target_compile_options(earshot_audio PRIVATE -O2 -Wall -Wextra)
