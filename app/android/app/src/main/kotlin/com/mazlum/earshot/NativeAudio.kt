package com.mazlum.earshot

import android.util.Log

/**
 * The one native library, loaded once.
 *
 * It carries libopus (compression) and RNNoise (noise suppression). Both used to ask for it
 * separately, which meant two lazy loaders racing to call `System.loadLibrary` for the same file
 * and two different stories about whether it was there.
 *
 * A build with no library for this processor is not an error anyone needs to see: the microphone
 * still works, uncompressed and unfiltered, exactly as it did before either existed.
 */
object NativeAudio {
    private const val TAG = "EarshotAudio"

    val available: Boolean by lazy {
        try {
            System.loadLibrary("earshot_audio")
            true
        } catch (e: UnsatisfiedLinkError) {
            Log.w(TAG, "no native audio on this build: ${e.message}")
            false
        }
    }
}
