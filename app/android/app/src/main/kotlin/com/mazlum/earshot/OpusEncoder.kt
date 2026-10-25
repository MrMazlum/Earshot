package com.mazlum.earshot

import android.util.Log

/**
 * Compresses one captured frame, so the microphone stops sending raw audio at ~770 kbps.
 *
 * Earshot sends a 20 ms frame every 20 ms. Uncompressed that is about 770 kbps for as long as the
 * mic is on, which on a shared home network is the single loudest thing Earshot does. The same
 * voice through Opus is 32-40 kbps, and the time the radio spends transmitting drops with it.
 *
 * ## It is allowed not to exist
 *
 * [create] returns null when the native library will not load — an ABI with no build, a stripped
 * APK, a phone this code has never seen. The caller falls back to sending raw audio, which is what
 * every build before this one did, and the receiver still understands it. **Compression is an
 * improvement, never a requirement**: a phone that cannot compress must still be a microphone.
 *
 * ## Thread rules
 *
 * One instance belongs to one thread — the capture thread. The underlying encoder carries state
 * about the audio it has already heard, so two threads encoding through one instance would produce
 * garbage. [close] must be called from that thread too, and nothing may call [encode] afterwards.
 */
class OpusEncoder private constructor(private var handle: Long, val rate: Int) {

    /**
     * Compresses `samples` from `pcm` and writes the packet into `out` at `outOffset`.
     *
     * `out` must have at least [MAX_PACKET_BYTES] of room past the offset, which is how the caller
     * can hand over the packet buffer with its header already written.
     *
     * @return bytes written, or -1 if the frame was refused. A refused frame is dropped by the
     *   caller rather than retried: it is 20 ms of audio, and the next one is already arriving.
     */
    fun encode(pcm: ShortArray, samples: Int, out: ByteArray, outOffset: Int): Int {
        if (handle == 0L) return -1
        val written = nativeEncode(handle, pcm, samples, out, outOffset)
        if (written < 0) {
            Log.w(TAG, "Opus refused a frame ($written)")
            return -1
        }
        return written
    }

    /** Releases the encoder. Safe to call twice; the second call does nothing. */
    fun close() {
        if (handle != 0L) {
            nativeDestroy(handle)
            handle = 0L
        }
    }

    companion object {
        private const val TAG = "EarshotOpus"

        /** The largest a single Opus packet can be, and so the room [encode] needs past its offset. */
        const val MAX_PACKET_BYTES = 1275

        /**
         * Bits per second. 32 kbps is transparent enough for a voice that the difference from
         * uncompressed is not the thing anyone will notice — and it is a twentieth of the traffic.
         */
        const val DEFAULT_BITRATE = 32_000

        /**
         * What the encoder is told to expect the network to lose, as a percentage.
         *
         * Not a measurement and not a promise: it is the budget the encoder spends on redundant
         * copies of each previous frame. At 0 it spends nothing and a lost frame is gone; too high
         * and it spends bits on insurance nobody claims. 5 is enough to cover the odd dropout on a
         * busy channel, which is the loss this is actually for.
         */
        const val EXPECTED_LOSS_PERCENT = 5

        /** True once the native library has loaded. False means every session sends raw audio. */
        val available: Boolean by lazy {
            if (!NativeAudio.available) return@lazy false
            Log.i(TAG, "libopus ${nativeVersion()}")
            true
        }

        /**
         * An encoder for a capture rate, or null if one cannot be made.
         *
         * Every rate Opus accepts is allowed here (8, 12, 16, 24 and 48 kHz), which is the reason
         * this is libopus and not Android's MediaCodec: the noise-cancelled voice source on many
         * phones only offers 16 kHz, and that has to compress too.
         */
        fun create(
            rate: Int,
            bitrate: Int = DEFAULT_BITRATE,
            expectedLossPercent: Int = EXPECTED_LOSS_PERCENT,
        ): OpusEncoder? {
            if (!available) return null
            if (!ACCEPTED_RATES.contains(rate)) {
                Log.w(TAG, "Opus does not take $rate Hz, sending raw audio instead")
                return null
            }
            val handle = try {
                nativeCreate(rate, bitrate, expectedLossPercent)
            } catch (e: UnsatisfiedLinkError) {
                Log.w(TAG, "Opus failed to start: ${e.message}")
                return null
            }
            if (handle == 0L) {
                Log.w(TAG, "Opus failed to start at $rate Hz")
                return null
            }
            return OpusEncoder(handle, rate)
        }

        /** The sample rates libopus accepts. Any other rate has to travel uncompressed. */
        val ACCEPTED_RATES = intArrayOf(8_000, 12_000, 16_000, 24_000, 48_000)

        @JvmStatic
        private external fun nativeCreate(rate: Int, bitrate: Int, expectedLossPercent: Int): Long

        @JvmStatic
        private external fun nativeEncode(
            handle: Long,
            pcm: ShortArray,
            samples: Int,
            out: ByteArray,
            outOffset: Int,
        ): Int

        @JvmStatic
        private external fun nativeDestroy(handle: Long)

        @JvmStatic
        private external fun nativeVersion(): String
    }
}
