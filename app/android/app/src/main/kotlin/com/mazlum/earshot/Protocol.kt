package com.mazlum.earshot

import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * The Earshot wire format. MUST stay byte-identical to the receiver's implementation.
 * Specified in protocol/README.md — change that first, then both ends, in the same commit.
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

    /**
     * The PC's reply, and the only packet that ever travels towards the phone.
     *
     * It is what makes "connected" answerable at all. A UDP `send` succeeds whether the datagram
     * reaches the PC or leaves down an interface with no route to it — which is exactly what
     * happens when the phone is on mobile data and the PC is a `192.168.x.x` address. Without a
     * reply the app cannot tell that apart from a working session, and it used to claim the latter.
     */
    const val TYPE_HELLO = 4

    const val FLAG_FEC = 0x01
    const val FLAG_ENC = 0x02
    const val FLAG_MARK = 0x04

    const val SAMPLE_RATE = 48000
    const val FRAME_MS = 20
    const val FRAME_SAMPLES = SAMPLE_RATE / 1000 * FRAME_MS // 960

    /** Frame samples for a rate other than 48k (the phone may force 16k — see the AudioSource trap). */
    fun frameSamples(rate: Int): Int = rate / 1000 * FRAME_MS

    /** What a [TYPE_HELLO] carries: an echo, and how much audio the PC is holding. */
    data class Hello(val sequence: Int, val bufferedMsX10: Int)

    /**
     * Reads a datagram as the PC's reply to *this* session, or returns null.
     *
     * Every field is checked before anything is believed, because this is the first time the app
     * has ever read from the network and the packet can come from anyone on the LAN. The ssrc test
     * is the one that matters: it is random per session, so a reply belonging to another phone —
     * or to this phone's previous run — is not evidence about the stream running now.
     *
     * The buffer depth arrives in tenths of a millisecond; see protocol/README.md, which is also
     * where the fields are documented as deliberately meaning something else in this one type.
     */
    fun parseHello(buf: ByteArray, len: Int, ssrc: Int): Hello? {
        if (len < HEADER_LEN) return null
        if (buf[0] != MAGIC_0 || buf[1] != MAGIC_1) return null
        val b2 = buf[2].toInt() and 0xFF
        if ((b2 shr 4) != VERSION) return null
        if ((b2 and 0x0F) != TYPE_HELLO) return null
        val bb = ByteBuffer.wrap(buf, 0, HEADER_LEN).order(ByteOrder.BIG_ENDIAN)
        val sequence = bb.getInt(4)
        val bufferedMsX10 = bb.getInt(8)
        if (bb.getInt(12) != ssrc) return null
        return Hello(sequence, bufferedMsX10)
    }

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
