package com.mazlum.earshot

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import java.net.Inet4Address

/**
 * What network the phone is on, and which address it holds there.
 *
 * This exists because of one real failure: the phone's Wi-Fi was off, its mobile data was on, and
 * the app streamed happily into nothing for a whole session. Every part of the app looked correct —
 * the pairing code resolved, the meter moved, the notification said "microphone live" — because a
 * UDP `send` to an unreachable private address succeeds exactly like one that arrives.
 *
 * A pairing code is an *address*, not a connection: it proves the digits were typed correctly and
 * nothing else. This is the other half of the answer; the PC's reply (Protocol.TYPE_HELLO) is the
 * first.
 *
 * Only the **default** network is watched, which is the one a socket will actually use. When Wi-Fi
 * is on it is the Wi-Fi; when Wi-Fi is off it is the mobile data that caused the bug.
 */
object NetworkWatch {

    const val WIFI = "wifi"
    const val CELLULAR = "cellular"
    const val ETHERNET = "ethernet"

    /** A VPN is up. LAN traffic usually still works, so this warns rather than blocks. */
    const val VPN = "vpn"
    const val NONE = "none"
    const val OTHER = "other"

    private var manager: ConnectivityManager? = null
    private var callback: ConnectivityManager.NetworkCallback? = null

    /** The last snapshot, so a UI attaching late does not have to wait for a change. */
    @Volatile
    private var latest: Map<String, Any?> = mapOf("transport" to NONE, "ip" to null, "prefix" to 0)

    fun snapshot(): Map<String, Any?> = latest

    /**
     * Starts watching. Safe to call twice — the second call is ignored rather than leaking a
     * second callback, because this is wired from the activity and activities are recreated.
     */
    fun start(context: Context) {
        if (callback != null) return
        val cm = context.applicationContext
            .getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return
        manager = cm

        val cb = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) = refresh()
            override fun onLost(network: Network) = refresh()
            override fun onCapabilitiesChanged(
                network: Network,
                capabilities: NetworkCapabilities,
            ) = refresh()

            override fun onLinkPropertiesChanged(network: Network, properties: LinkProperties) =
                refresh()
        }
        callback = cb
        try {
            cm.registerDefaultNetworkCallback(cb)
        } catch (_: Throwable) {
            // A phone that refuses the callback still gets the polled snapshot below, which is
            // what the UI reads on every screen build anyway.
            callback = null
        }
        refresh()
    }

    fun stop() {
        val cm = manager
        val cb = callback
        if (cm != null && cb != null) {
            try {
                cm.unregisterNetworkCallback(cb)
            } catch (_: Throwable) {
            }
        }
        callback = null
        manager = null
    }

    /** Reads the current state and tells the UI if it changed. */
    fun refresh() {
        val next = read()
        if (next != latest) {
            latest = next
            Bus.emitNetwork(next)
        }
    }

    private fun read(): Map<String, Any?> {
        val cm = manager ?: return mapOf("transport" to NONE, "ip" to null, "prefix" to 0)
        val network = cm.activeNetwork
        val caps = network?.let { cm.getNetworkCapabilities(it) }
        if (network == null || caps == null) {
            return mapOf("transport" to NONE, "ip" to null, "prefix" to 0)
        }

        // VPN first, and deliberately: when a tunnel is up it *is* the default network, and saying
        // "Wi-Fi" then would hide the thing most likely to be eating the packets.
        val transport = when {
            caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN) -> VPN
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> WIFI
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> ETHERNET
            caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> CELLULAR
            else -> OTHER
        }

        // The IPv4 address on that network, with its prefix length: together they are the phone's
        // subnet, which is what says whether the PC is somewhere it could possibly be reached.
        var ip: String? = null
        var prefix = 0
        val link = cm.getLinkProperties(network)
        if (link != null) {
            for (address in link.linkAddresses) {
                val inet = address.address
                if (inet is Inet4Address && !inet.isLoopbackAddress && !inet.isLinkLocalAddress) {
                    ip = inet.hostAddress
                    prefix = address.prefixLength
                    break
                }
            }
        }
        return mapOf("transport" to transport, "ip" to ip, "prefix" to prefix)
    }
}
