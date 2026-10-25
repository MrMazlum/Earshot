package com.mazlum.earshot

import android.util.Log

/**
 * Strips steady background noise out of a captured frame — fans, hum, traffic, a laptop on a desk.
 *
 * RNNoise, from the same people as Opus and under the same licence. It is a small trained model
 * that decides, band by band, how much of what it is hearing is voice, and turns the rest down.
 *
 * ## Why this exists when the phone already has a noise suppressor
 *
 * It does, in the voice-call capture chain — the "Voice call" source. The catch is that the chain
 * is a package deal: on many phones it only offers **16 kHz**, which costs the top of a voice. This
 * runs on the full-band signal instead, so the choice stops being *clean* or *48 kHz*.
 *
 * ## Its two rules, which come from RNNoise and not from us
 *
 * 1. **48 kHz only.** At any other capture rate this is not used at all. Resampling into it and
 *    back out would cost more than the noise does.
 * 2. **Whole 10 ms frames**, fed in order, because the model carries state between them. One
 *    Earshot frame is exactly two of them.
 *
 * Like [OpusEncoder], it is allowed not to exist: [create] returns null when the native library
 * will not load, and the microphone carries on as it always did.
 *
 * One instance belongs to one thread — the capture thread — for the same reason the encoder does.
 */
class Denoiser private constructor(private var handle: Long) {

    /**
     * Cleans `samples` of `pcm` in place.
     *
     * @return true if the frame was processed. False means it was left untouched, which is a
     *   frame of ordinary audio rather than a failure worth stopping for.
     */
    fun process(pcm: ShortArray, samples: Int): Boolean {
        if (handle == 0L) return false
        return nativeProcess(handle, pcm, samples) > 0
    }

    /** Releases the denoiser. Safe to call twice. */
    fun close() {
        if (handle != 0L) {
            nativeDestroy(handle)
            handle = 0L
        }
    }

    companion object {
        private const val TAG = "EarshotDenoise"

        /** The only capture rate RNNoise takes. */
        const val REQUIRED_RATE = 48_000

        /**
         * True when noise suppression can run at this capture rate and this frame length.
         *
         * Both conditions are RNNoise's own. Checked out loud rather than assumed, so a future
         * change to the frame length fails here instead of sounding wrong.
         */
        fun usableAt(rate: Int, frameSamples: Int): Boolean {
            if (!NativeAudio.available) return false
            if (rate != REQUIRED_RATE) return false
            val frame = try {
                nativeFrameSize()
            } catch (e: UnsatisfiedLinkError) {
                return false
            }
            return frame > 0 && frameSamples % frame == 0
        }

        /** A denoiser, or null if one cannot be made. */
        fun create(rate: Int, frameSamples: Int): Denoiser? {
            if (!usableAt(rate, frameSamples)) {
                Log.i(TAG, "no noise suppression at $rate Hz")
                return null
            }
            val handle = try {
                nativeCreate()
            } catch (e: UnsatisfiedLinkError) {
                return null
            }
            if (handle == 0L) {
                Log.w(TAG, "RNNoise failed to start")
                return null
            }
            Log.i(TAG, "noise suppression on, ${nativeFrameSize()} samples a frame")
            return Denoiser(handle)
        }

        @JvmStatic
        private external fun nativeCreate(): Long

        @JvmStatic
        private external fun nativeProcess(handle: Long, pcm: ShortArray, samples: Int): Int

        @JvmStatic
        private external fun nativeDestroy(handle: Long)

        @JvmStatic
        private external fun nativeFrameSize(): Int
    }
}
