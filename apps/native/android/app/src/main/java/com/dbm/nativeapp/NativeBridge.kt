package com.dbm.nativeapp

import android.content.Context
import android.util.Base64
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import java.io.File
import java.security.KeyStore
import javax.net.ssl.TrustManagerFactory
import javax.net.ssl.X509TrustManager

internal object NativeBridge {
    init { System.loadLibrary("dbm_android") }
    external fun create(path: ByteArray): Long
    external fun createDemo(): Long
    external fun call(handle: Long, request: ByteArray): ByteArray?
    external fun free(handle: Long)
}

class BridgeClient(private val context: Context, private val io: CoroutineDispatcher = Dispatchers.IO.limitedParallelism(1)) {
    @PublishedApi internal val json = Json { ignoreUnknownKeys = true }
    private var handle = 0L

    suspend fun open(demo: Boolean = false) = withContext(io) {
        if (handle != 0L) return@withContext
        handle = if (demo) {
            check(BuildConfig.ALLOW_DEMO) { "Demo mode is debug-only" }; NativeBridge.createDemo()
        } else NativeBridge.create(File(context.filesDir, "dbm-mobile.sqlite3").absolutePath.encodeToByteArray())
        check(handle != 0L) { "Unable to create bridge session" }
    }

    suspend fun call(request: JsonObject): JsonElement = withContext(io) {
        val current = handle
        check(current != 0L) { "Session is closed" }
        val reply = NativeBridge.call(current, request.toString().encodeToByteArray())
            ?: error("Bridge returned no response")
        val envelope = json.parseToJsonElement(reply.decodeToString()).jsonObject
        if (envelope["ok"]?.jsonPrimitive?.booleanOrNull != true) error(envelope["error"]?.jsonPrimitive?.content ?: "Bridge call failed")
        envelope["value"] ?: JsonNull
    }

    internal suspend inline fun <reified T> decode(request: JsonObject): T = json.decodeFromJsonElement(call(request))
    suspend fun close() = withContext(io) { handle.takeIf { it != 0L }?.let(NativeBridge::free); handle = 0 }

    fun writePlatformCaBundle(): String {
        val factory = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm())
        factory.init(null as KeyStore?)
        val certificates = factory.trustManagers.filterIsInstance<X509TrustManager>().flatMap { it.acceptedIssuers.toList() }
        check(certificates.isNotEmpty()) { "Android trust store has no accepted issuers" }
        val target = File(context.filesDir, "android-ca-bundle.pem")
        target.outputStream().bufferedWriter(Charsets.US_ASCII).use { out ->
            certificates.forEach { certificate ->
                out.appendLine("-----BEGIN CERTIFICATE-----")
                out.appendLine(Base64.encodeToString(certificate.encoded, Base64.NO_WRAP).chunked(64).joinToString("\n"))
                out.appendLine("-----END CERTIFICATE-----")
            }
        }
        return target.absolutePath
    }
}
