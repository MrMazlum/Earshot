package com.mazlum.earshot

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {

    private companion object {
        const val METHOD_CHANNEL = "earshot/control"
        const val EVENT_CHANNEL = "earshot/events"
        const val PREFS = "earshot_prefs"
        const val PERM_REQUEST = 4711
        /** Whether the microphone prompt has ever been shown. See [micPermissionState]. */
        const val ASKED_MIC = "asked_mic"
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        EventChannel(flutterEngine.dartExecutor.binaryMessenger, EVENT_CHANNEL).setStreamHandler(
            object : EventChannel.StreamHandler {
                override fun onListen(arguments: Any?, events: EventChannel.EventSink?) {
                    Bus.sink = { event -> events?.success(event) }
                }

                override fun onCancel(arguments: Any?) {
                    Bus.sink = null
                }
            }
        )

        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, METHOD_CHANNEL)
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "requestPermissions" -> result.success(requestNeededPermissions())
                    "hasPermissions" -> result.success(hasMicPermission())
                    "micPermissionState" -> result.success(micPermissionState())
                    "openAppSettings" -> {
                        startActivity(
                            Intent(
                                Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                                Uri.fromParts("package", packageName, null)
                            )
                        )
                        result.success(true)
                    }
                    "isRunning" -> result.success(Bus.running)

                    "start" -> {
                        if (!hasMicPermission()) {
                            result.error("no_permission", "Microphone permission not granted", null)
                        } else {
                            val intent = Intent(this, MicService::class.java).apply {
                                action = MicService.ACTION_START
                                putExtra(MicService.EXTRA_HOST, call.argument<String>("host"))
                                putExtra(MicService.EXTRA_PORT, call.argument<Int>("port") ?: 47811)
                                putExtra(MicService.EXTRA_SOURCE, call.argument<Int>("source") ?: 7)
                                putExtra(MicService.EXTRA_RATE, call.argument<Int>("rate") ?: 48000)
                            }
                            ContextCompat.startForegroundService(this, intent)
                            result.success(true)
                        }
                    }

                    "stop" -> {
                        startService(Intent(this, MicService::class.java).setAction(MicService.ACTION_STOP))
                        result.success(true)
                    }

                    "getPrefs" -> {
                        val p = getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                        result.success(
                            mapOf(
                                "host" to p.getString("host", ""),
                                "port" to p.getInt("port", 47811),
                                "source" to p.getInt("source", 7),
                                "rate" to p.getInt("rate", 48000),
                                // The pairing code is kept as typed so the field comes back the way
                                // the user left it; host and port above are what it resolved to.
                                "code" to p.getString("code", ""),
                                "manual" to p.getBoolean("manual", false),
                            )
                        )
                    }

                    "setPrefs" -> {
                        getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().apply {
                            call.argument<String>("host")?.let { putString("host", it) }
                            call.argument<Int>("port")?.let { putInt("port", it) }
                            call.argument<Int>("source")?.let { putInt("source", it) }
                            call.argument<Int>("rate")?.let { putInt("rate", it) }
                            call.argument<String>("code")?.let { putString("code", it) }
                            call.argument<Boolean>("manual")?.let { putBoolean("manual", it) }
                        }.apply()
                        result.success(true)
                    }

                    else -> result.notImplemented()
                }
            }
    }

    private fun hasMicPermission(): Boolean =
        ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED

    /**
     * "granted", "askable", or "blocked".
     *
     * "blocked" is the case worth having a name for: after two refusals Android stops showing the
     * dialog at all, so `requestPermissions` returns without anything appearing on screen. Telling
     * someone to "press Start again" then is advice that can never work, and the only way out is
     * the system settings page.
     *
     * `shouldShowRequestPermissionRationale` is false both before the first ask *and* after a
     * permanent refusal, so it cannot tell those apart by itself. Hence the stored flag.
     */
    private fun micPermissionState(): String = when {
        hasMicPermission() -> "granted"
        ActivityCompat.shouldShowRequestPermissionRationale(
            this, Manifest.permission.RECORD_AUDIO
        ) -> "askable"
        getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(ASKED_MIC, false) -> "blocked"
        else -> "askable"
    }

    /** Returns true if everything is already granted; otherwise fires the system prompts. */
    private fun requestNeededPermissions(): Boolean {
        val wanted = mutableListOf<String>()
        if (!hasMicPermission()) {
            wanted += Manifest.permission.RECORD_AUDIO
            // Remembered before the prompt, not after: whether it was granted is a separate
            // question, and this only records that the system has had its one chance to ask.
            getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .edit().putBoolean(ASKED_MIC, true).apply()
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            wanted += Manifest.permission.POST_NOTIFICATIONS
        }
        if (wanted.isNotEmpty()) {
            ActivityCompat.requestPermissions(this, wanted.toTypedArray(), PERM_REQUEST)
            return false
        }
        return true
    }
}
