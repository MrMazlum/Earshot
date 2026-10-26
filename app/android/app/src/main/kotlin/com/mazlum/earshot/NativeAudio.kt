package com.mazlum.earshot

import android.util.Log

/**
 * The one native library, loaded once.
 *
 * It carries libopus, for compression. It used to carry a noise suppressor as well, until that
 * job moved to the PC, where there is the processor for a model that can handle a keyboard.
 *
 * A build with no library for this processor is not an error anyone needs to see: the microphone
 * still works, uncompressed, exactly as it did before the library existed.
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
