package com.mazlum.earshot

import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * The Earshot wire format. MUST stay byte-identical to the receiver's implementation.
 * Source of truth: ~/EarshotBrain/05-Wire-Protocol.md §4 — change there first, then both ends.
 *
 *  0               1               2               3
 *  +-------+-------+-------+-------+
 *  | 'E'   | 'S'   | v|type| flags |
 *  +-------+-------+-------+-------+
 *  |         sequence (u32)        |
 *  +-------------------------------+
 *  |  timestamp (u32, samples)     |
 *  +-------------------------------+
 *  |          ssrc (u32)           |
 *  +-------------------------------+
 *  |   payload (Opus, or raw PCM in debug mode)
 */
object Protocol {
    const val MAGIC_0: Byte = 0x45 // 'E'
    const val MAGIC_1: Byte = 0x53 // 'S'
    const val VERSION = 1
    const val HEADER_LEN = 16

    const val TYPE_OPUS = 0
    const val TYPE_DTX = 1
    const val TYPE_KEEPALIVE = 2
    const val TYPE_PCM_DEBUG = 3 // s16le mono — dev builds only, never a release

    const val FLAG_FEC = 0x01
    const val FLAG_ENC = 0x02
    const val FLAG_MARK = 0x04

    const val SAMPLE_RATE = 48000
    const val FRAME_MS = 20
    const val FRAME_SAMPLES = SAMPLE_RATE / 1000 * FRAME_MS // 960

    /** Frame samples for a rate other than 48k (the phone may force 16k — see the AudioSource trap). */
    fun frameSamples(rate: Int): Int = rate / 1000 * FRAME_MS

    /**
     * Writes the 16-byte header into [out] at offset 0. Big-endian (network order).
     * Returns the number of bytes written.
     */
    fun writeHeader(
        out: ByteArray,
        type: Int,
        flags: Int,
        sequence: Int,
        timestamp: Int,
        ssrc: Int,
    ): Int {
        val bb = ByteBuffer.wrap(out).order(ByteOrder.BIG_ENDIAN)
        bb.put(MAGIC_0)
        bb.put(MAGIC_1)
        bb.put((((VERSION and 0x0F) shl 4) or (type and 0x0F)).toByte())
        bb.put((flags and 0xFF).toByte())
        bb.putInt(sequence)
        bb.putInt(timestamp)
        bb.putInt(ssrc)
        return HEADER_LEN
    }
}
