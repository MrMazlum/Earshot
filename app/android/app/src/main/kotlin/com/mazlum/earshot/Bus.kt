package com.mazlum.earshot

import android.os.Handler
import android.os.Looper

/**
 * One-way channel from the capture thread to the Flutter UI.
 *
 * The capture thread must never touch Flutter directly (see Rules/no-blocking-audio-thread.md):
 * it drops a map here at 5 Hz and moves on; delivery hops to the main thread.
 */
object Bus {
    private val main = Handler(Looper.getMainLooper())

    @Volatile var sink: ((Map<String, Any?>) -> Unit)? = null

    /** Last known state, so the UI can recover it after a rotation or a reattach. */
    @Volatile var running: Boolean = false
        private set

    private fun post(event: Map<String, Any?>) {
        val s = sink ?: return
        main.post { s(event) }
    }

    fun emitStarted(rate: Int, source: Int) {
        running = true
        post(mapOf("event" to "started", "rate" to rate, "source" to source))
    }

    fun emitStats(packets: Long, bytes: Long, level: Float, rate: Int, source: Int) {
        post(
            mapOf(
                "event" to "stats",
                "packets" to packets,
                "bytes" to bytes,
                "level" to level,
                "rate" to rate,
                "source" to source,
            )
        )
    }

    fun emitError(message: String) {
        post(mapOf("event" to "error", "message" to message))
    }

    fun emitStopped() {
        running = false
        post(mapOf("event" to "stopped"))
    }
}
