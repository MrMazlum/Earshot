package com.mazlum.earshot

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.AudioFormat
import android.media.AudioRecord
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import kotlin.concurrent.thread
import kotlin.math.abs
import kotlin.random.Random

/**
 * Captures the microphone and streams it to the PC receiver over UDP.
 *
 * A foreground service because Android will not let a backgrounded app hold the mic, and because
 * the user must always be able to see that something is listening.
 *
 * Raw PCM for now (Protocol.TYPE_PCM_DEBUG); Opus comes next.
 *
 * The rule that governs this file is the audio-thread rule in CONTRIBUTING.md: the read loop does
 * capture → header → send and nothing else. No logging, no allocation per frame, no UI work.
 */
class MicService : Service() {

    companion object {
        const val ACTION_START = "com.mazlum.earshot.START"
        const val ACTION_STOP = "com.mazlum.earshot.STOP"
        const val ACTION_MUTE = "com.mazlum.earshot.MUTE"
        const val EXTRA_HOST = "host"
        const val EXTRA_PORT = "port"
        const val EXTRA_SOURCE = "source"
        const val EXTRA_RATE = "rate"
        const val EXTRA_MUTED = "muted"

        private const val CHANNEL_ID = "earshot_session"
        private const val NOTIF_ID = 1

        /** How often a keepalive goes out while muted. Protocol §7 says 1 s. */
        private const val KEEPALIVE_NS = 1_000_000_000L
    }

    @Volatile private var running = false

    /**
     * Read by the capture thread once per frame, written by whoever taps Mute.
     *
     * Volatile rather than a lock on purpose: the capture thread must never wait on anything
     * (Rules/no-blocking-audio-thread). A one-frame delay in noticing the flag is 20 ms and does
     * not matter; a priority inversion would.
     */
    @Volatile private var muted = false

    private var worker: Thread? = null
    private var wifiLock: WifiManager.WifiLock? = null

    /** Kept so the notification can be rebuilt when mute flips without re-reading the intent. */
    private var streamHost: String = ""
    private var streamPort: Int = 0

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopStreaming()
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_MUTE -> {
                // Only meaningful during a session. Arriving while idle is harmless — and must not
                // call startForeground, exactly as ACTION_STOP does not.
                if (running) setMuted(intent.getBooleanExtra(EXTRA_MUTED, !muted))
                return START_NOT_STICKY
            }
            ACTION_START -> {
                val host = intent.getStringExtra(EXTRA_HOST) ?: return START_NOT_STICKY
                val port = intent.getIntExtra(EXTRA_PORT, 47811)
                val source = intent.getIntExtra(EXTRA_SOURCE, 7) // VOICE_COMMUNICATION
                val rate = intent.getIntExtra(EXTRA_RATE, Protocol.SAMPLE_RATE)
                streamHost = host
                streamPort = port
                // Every session starts live. A mute carried over from last time would be a silent
                // microphone that nobody asked for.
                muted = false
                startForegroundCompat()
                startStreaming(host, port, source, rate)
            }
        }
        return START_STICKY
    }

    override fun onDestroy() {
        stopStreaming()
        super.onDestroy()
    }

    // ── Foreground plumbing ──────────────────────────────────────────────────

    private fun startForegroundCompat() {
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "Earshot session", NotificationManager.IMPORTANCE_LOW)
                    .apply { description = "Shown while your microphone is being streamed to your PC" }
            )
        }
        val notif = buildNotification()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIF_ID, notif, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
        } else {
            startForeground(NOTIF_ID, notif)
        }
    }

    /**
     * The session notification, reflecting [muted].
     *
     * The notification is the only honest answer to "is this thing listening to me right now",
     * because it is visible when the app is not (10-Working-Rules: the mic is live → persistent
     * notification, visible mute). So mute is stated in the title, not merely offered as a button.
     */
    private fun buildNotification(): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, MicService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        // The target state is written into the extra rather than left as a toggle, so a stale
        // notification cannot flip mute the wrong way. FLAG_UPDATE_CURRENT refreshes it because
        // this is rebuilt on every change.
        val mute = PendingIntent.getService(
            this, 2,
            Intent(this, MicService::class.java)
                .setAction(ACTION_MUTE)
                .putExtra(EXTRA_MUTED, !muted),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        return Notification.Builder(this, CHANNEL_ID)
            .setContentTitle(
                if (muted) "Earshot — muted" else "Earshot — microphone live"
            )
            .setContentText(
                if (muted) "Nothing is being sent to $streamHost:$streamPort"
                else "Streaming to $streamHost:$streamPort"
            )
            .setSmallIcon(
                if (muted) android.R.drawable.ic_lock_silent_mode
                else android.R.drawable.ic_btn_speak_now
            )
            .setOngoing(true)
            .setContentIntent(open)
            .addAction(
                Notification.Action.Builder(null, if (muted) "Unmute" else "Mute", mute).build()
            )
            .addAction(Notification.Action.Builder(null, "Stop", stop).build())
            .build()
    }

    /**
     * Flips the gate. Called on the main thread only — the capture thread reads [muted] but never
     * writes it, and never touches the notification or the Bus's main-thread handler.
     */
    private fun setMuted(value: Boolean) {
        if (muted == value) return
        muted = value
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        nm.notify(NOTIF_ID, buildNotification())
        Bus.emitMuted(value)
    }

    private fun acquireWifiLock() {
        val wm = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            WifiManager.WIFI_MODE_FULL_LOW_LATENCY
        } else {
            @Suppress("DEPRECATION") WifiManager.WIFI_MODE_FULL_HIGH_PERF
        }
        wifiLock = wm.createWifiLock(mode, "earshot:stream").apply { acquire() }
    }

    // ── The stream ───────────────────────────────────────────────────────────

    private fun startStreaming(host: String, port: Int, source: Int, requestedRate: Int) {
        if (running) return
        running = true
        acquireWifiLock()

        worker = thread(name = "earshot-capture", priority = Thread.MAX_PRIORITY) {
            var record: AudioRecord? = null
            var socket: DatagramSocket? = null
            try {
                // Try the requested rate, then 16 kHz. Some devices only give the noise-cancelled
                // voice chain at 16 kHz — that is exactly what P0.1 is measuring.
                var rate = requestedRate
                var rec = openRecorder(source, rate)
                if (rec == null && rate != 16000) {
                    rate = 16000
                    rec = openRecorder(source, rate)
                }
                if (rec == null) {
                    Bus.emitError("Could not open the microphone (source=$source). Another app may be using it.")
                    return@thread
                }
                record = rec

                val frameSamples = Protocol.frameSamples(rate)
                val payloadLen = frameSamples * 2
                val packet = ByteArray(Protocol.HEADER_LEN + payloadLen)
                val addr = InetAddress.getByName(host)
                val sock = DatagramSocket().apply { trafficClass = 0x10 /* IPTOS_LOWDELAY */ }
                socket = sock
                val dgram = DatagramPacket(packet, packet.size, addr, port)
                val ssrc = Random.nextInt()

                // Muting sends these instead of audio, so the receiver's "connected" state and the
                // NAT/firewall hole survive the silence (Protocol §7). Allocated once: the loop
                // below must not allocate.
                val keepalive = ByteArray(Protocol.HEADER_LEN)
                val keepaliveDgram = DatagramPacket(keepalive, keepalive.size, addr, port)

                var sequence = 0
                var timestamp = 0
                var packets = 0L
                var bytes = 0L
                var peak = 0
                var lastReport = System.nanoTime()

                // Mute bookkeeping, all thread-local — `muted` itself is the only shared word.
                var wasMuted = false
                var lastKeepalive = 0L
                var keepaliveDue = false
                var markNext = false

                record.startRecording()
                Bus.emitStarted(rate, source)

                while (running) {
                    // Fill exactly one frame. AudioRecord.read blocks until data is available.
                    var off = Protocol.HEADER_LEN
                    val end = Protocol.HEADER_LEN + payloadLen
                    var failed = false
                    while (off < end) {
                        val n = record.read(packet, off, end - off)
                        if (n <= 0) { failed = true; break }
                        off += n
                    }
                    if (failed) {
                        Bus.emitError("Microphone read failed — capture stopped.")
                        break
                    }

                    // The frame is always read, even while muted: the blocking read is what paces
                    // this loop at 20 ms, and letting AudioRecord's buffer overflow instead would
                    // hand back stale audio the moment the user unmutes.
                    val gated = muted
                    if (gated != wasMuted) {
                        wasMuted = gated
                        // Tell the receiver immediately in either direction: a keepalive the
                        // instant we go quiet, and MARK on the first frame of speech after it.
                        if (gated) keepaliveDue = true else markNext = true
                    }

                    if (gated) {
                        // The gate. The frame is simply never sent — the samples do not leave the
                        // phone, which is the only thing that makes this button worth trusting.
                        //
                        // `sequence` deliberately does NOT advance. It is what the receiver's
                        // reorder buffer counts by, so a frozen counter makes a mute look like a
                        // contiguous stream that paused, rather than as many lost packets as the
                        // mute was long.
                        //
                        // `timestamp` does advance: it is a sample clock, and Protocol §4 has it
                        // surviving gaps.
                        timestamp += frameSamples

                        val now = System.nanoTime()
                        if (keepaliveDue || now - lastKeepalive >= KEEPALIVE_NS) {
                            Protocol.writeHeader(
                                keepalive,
                                type = Protocol.TYPE_KEEPALIVE,
                                flags = 0,
                                sequence = sequence,
                                timestamp = timestamp,
                                ssrc = ssrc,
                            )
                            sock.send(keepaliveDgram)
                            lastKeepalive = now
                            keepaliveDue = false
                        }

                        // A meter that still twitched while muted would make a mute bug look
                        // normal, which is the one failure this feature cannot afford.
                        peak = 0
                        if (now - lastReport > 200_000_000L) {
                            Bus.emitStats(packets, bytes, 0f, rate, source)
                            lastReport = now
                        }
                        continue
                    }

                    Protocol.writeHeader(
                        packet,
                        type = Protocol.TYPE_PCM_DEBUG,
                        flags = if (markNext) Protocol.FLAG_MARK else 0,
                        sequence = sequence,
                        timestamp = timestamp,
                        ssrc = ssrc,
                    )
                    sock.send(dgram)
                    markNext = false

                    sequence++
                    timestamp += frameSamples
                    packets++
                    bytes += packet.size

                    // Cheap peak meter: every 4th sample is plenty for a UI bar.
                    var i = Protocol.HEADER_LEN
                    while (i < end - 1) {
                        val s = (packet[i].toInt() and 0xFF) or (packet[i + 1].toInt() shl 8)
                        val v = abs(s.toShort().toInt())
                        if (v > peak) peak = v
                        i += 8
                    }

                    val now = System.nanoTime()
                    if (now - lastReport > 200_000_000L) { // 5 Hz — off the hot path, cheap
                        Bus.emitStats(packets, bytes, peak / 32768f, rate, source)
                        peak = 0
                        lastReport = now
                    }
                }
            } catch (t: Throwable) {
                Bus.emitError(t.message ?: t.javaClass.simpleName)
            } finally {
                try { record?.stop() } catch (_: Throwable) {}
                record?.release()
                socket?.close()
                Bus.emitStopped()
            }
        }
    }

    private fun openRecorder(source: Int, rate: Int): AudioRecord? {
        val minBuf = AudioRecord.getMinBufferSize(
            rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT
        )
        if (minBuf <= 0) return null
        // Four frames of headroom: enough to survive a scheduling hiccup, small enough to stay
        // inside the latency budget - 4 x 20 ms is 80 ms if it ever actually fills.
        val bufSize = maxOf(minBuf, Protocol.frameSamples(rate) * 2 * 4)
        return try {
            val r = AudioRecord(source, rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, bufSize)
            if (r.state == AudioRecord.STATE_INITIALIZED) r else { r.release(); null }
        } catch (_: Throwable) {
            null
        }
    }

    private fun stopStreaming() {
        running = false
        worker?.join(500)
        worker = null
        try { wifiLock?.takeIf { it.isHeld }?.release() } catch (_: Throwable) {}
        wifiLock = null
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) stopForeground(STOP_FOREGROUND_REMOVE)
        else @Suppress("DEPRECATION") stopForeground(true)
    }
}
