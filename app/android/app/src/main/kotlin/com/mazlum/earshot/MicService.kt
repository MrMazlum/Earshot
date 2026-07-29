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
 * Phase P1: raw PCM (Protocol.TYPE_PCM_DEBUG). Opus comes next — see ~/EarshotBrain/MASTER_ROADMAP.md.
 * Rule that governs this file: ~/EarshotBrain/Rules/no-blocking-audio-thread.md. The read loop does
 * capture → header → send and nothing else. No logging, no allocation per frame, no UI work.
 */
class MicService : Service() {

    companion object {
        const val ACTION_START = "com.mazlum.earshot.START"
        const val ACTION_STOP = "com.mazlum.earshot.STOP"
        const val EXTRA_HOST = "host"
        const val EXTRA_PORT = "port"
        const val EXTRA_SOURCE = "source"
        const val EXTRA_RATE = "rate"

        private const val CHANNEL_ID = "earshot_session"
        private const val NOTIF_ID = 1
    }

    @Volatile private var running = false
    private var worker: Thread? = null
    private var wifiLock: WifiManager.WifiLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopStreaming()
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_START -> {
                val host = intent.getStringExtra(EXTRA_HOST) ?: return START_NOT_STICKY
                val port = intent.getIntExtra(EXTRA_PORT, 47811)
                val source = intent.getIntExtra(EXTRA_SOURCE, 7) // VOICE_COMMUNICATION
                val rate = intent.getIntExtra(EXTRA_RATE, Protocol.SAMPLE_RATE)
                startForegroundCompat(host, port)
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

    private fun startForegroundCompat(host: String, port: Int) {
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "Earshot session", NotificationManager.IMPORTANCE_LOW)
                    .apply { description = "Shown while your microphone is being streamed to your PC" }
            )
        }
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, MicService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        val notif: Notification = Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("Earshot — microphone live")
            .setContentText("Streaming to $host:$port")
            .setSmallIcon(android.R.drawable.ic_btn_speak_now)
            .setOngoing(true)
            .setContentIntent(open)
            .addAction(Notification.Action.Builder(null, "Stop", stop).build())
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIF_ID, notif, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
        } else {
            startForeground(NOTIF_ID, notif)
        }
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

                var sequence = 0
                var timestamp = 0
                var packets = 0L
                var bytes = 0L
                var peak = 0
                var lastReport = System.nanoTime()

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

                    Protocol.writeHeader(
                        packet,
                        type = Protocol.TYPE_PCM_DEBUG,
                        flags = 0,
                        sequence = sequence,
                        timestamp = timestamp,
                        ssrc = ssrc,
                    )
                    sock.send(dgram)

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
        // inside the latency budget (~06-Latency-Budget.md stage 1).
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
