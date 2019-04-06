/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package mozilla.appservices.httpconfig

import com.google.protobuf.ByteString
import mozilla.appservices.support.RustBuffer
import mozilla.components.concept.fetch.Client
import mozilla.components.concept.fetch.MutableHeaders
import mozilla.components.concept.fetch.Request
import java.io.InputStream
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.ReentrantReadWriteLock
import kotlin.concurrent.read
import kotlin.concurrent.write

/**
 * Singleton allowing management of the HTTP backend
 * used by Rust components.
 */
object RustHttpConfig {
    // Protects imp/client
    private var lock = ReentrantReadWriteLock()
    @Volatile
    private var client: Lazy<Client>? = null
    // Important note to future maintainers: if you mess around with
    // this code, you have to make sure `imp` can't get GCed. Extremely
    // bad things will happen if it does!
    @Volatile
    private var imp: CallbackImpl? = null

    /**
     * Set the HTTP client to be used by all Rust code.
     * the `Lazy`'s value is not read until the first request is made.
     */
    @Synchronized
    fun setClient(c: Lazy<Client>) {
        lock.write {
            client = c
            if (imp == null) {
                imp = CallbackImpl()
                LibViaduct.INSTANCE.viaduct_initialize(imp!!)
            }
        }
    }

    internal fun convertRequest(request: MsgTypes.Request): Request {
        val headers = MutableHeaders()
        for (h in request.headersMap) {
            headers.append(h.key, h.value)
        }
        return Request(
                url = request.url,
                method = convertMethod(request.method),
                headers = headers,
                connectTimeout = Pair(request.connectTimeoutSecs.toLong(), TimeUnit.SECONDS),
                readTimeout = Pair(request.readTimeoutSecs.toLong(), TimeUnit.SECONDS),
                body = if (request.hasBody()) {
                    Request.Body(ByteStringInputStream(request.body))
                } else {
                    null
                },
                redirect = if (request.followRedirects) {
                    Request.Redirect.FOLLOW
                } else {
                    Request.Redirect.MANUAL
                },
                cookiePolicy = if (request.includeCookies) {
                    Request.CookiePolicy.INCLUDE
                } else {
                    Request.CookiePolicy.OMIT
                },
                useCaches = request.useCaches
        )
    }

    @Suppress("TooGenericExceptionCaught", "ReturnCount")
    internal fun doFetch(b: RustBuffer.ByValue): RustBuffer.ByValue {
        lock.read {
            try {
                val request = MsgTypes.Request.parseFrom(b.asCodedInputStream())
                val rb = try {
                    // Note: `client!!` is fine here, since if client is null,
                    // we wouldn't have yet initialized
                    val resp = client!!.value.fetch(convertRequest(request))
                    val rb = MsgTypes.Response.newBuilder()
                            .setUrl(resp.url)
                            .setStatus(resp.status)
                            .setBody(resp.body.useStream {
                                ByteString.readFrom(it)
                            })

                    for (h in resp.headers) {
                        rb.putHeaders(h.name, h.value)
                    }
                    rb
                } catch (e: Throwable) {
                    MsgTypes.Response.newBuilder().setException(
                            MsgTypes.Response.ExceptionThrown.newBuilder()
                                    .setName(e.javaClass.canonicalName)
                                    .setMsg(e.message))
                }
                val built = rb.build()
                val needed = built.serializedSize
                val outputBuf = LibViaduct.INSTANCE.viaduct_alloc_bytebuffer(needed)
                try {
                    // This is only null if we passed a negative number or something to
                    // viaduct_alloc_bytebuffer.
                    val stream = outputBuf.asCodedOutputStream()!!
                    built.writeTo(stream)
                    return outputBuf
                } catch (e: Throwable) {
                    // Note: we want to clean this up only if we are not returning it to rust.
                    LibViaduct.INSTANCE.viaduct_destroy_bytebuffer(outputBuf)
                    throw e
                }
            } finally {
                LibViaduct.INSTANCE.viaduct_destroy_bytebuffer(b)
            }
        }
    }
}

internal fun convertMethod(m: MsgTypes.Request.Method): Request.Method {
    return when (m) {
        MsgTypes.Request.Method.GET -> Request.Method.GET
        MsgTypes.Request.Method.POST -> Request.Method.POST
        MsgTypes.Request.Method.HEAD -> Request.Method.HEAD
        MsgTypes.Request.Method.OPTIONS -> Request.Method.OPTIONS
        MsgTypes.Request.Method.DELETE -> Request.Method.DELETE
        MsgTypes.Request.Method.PUT -> Request.Method.PUT
        MsgTypes.Request.Method.TRACE -> Request.Method.TRACE
        MsgTypes.Request.Method.CONNECT -> Request.Method.CONNECT
    }
}

internal class CallbackImpl : RawFetchCallback {
    @Suppress("TooGenericExceptionCaught")
    override fun invoke(b: RustBuffer.ByValue): RustBuffer.ByValue {
        try {
            return RustHttpConfig.doFetch(b)
        } catch (e: Throwable) {
            // This is our last resort. It's bad news should we fail to
            // return something from this function.
            return RustBuffer.ByValue()
        }
    }
}

// The protobuf `bytes` type comes over as a com.google.protobuf.ByteString.
// There's no provided way to convert/wrap this to an InputStream, so we do
// that manually to avoid extra copying.
internal class ByteStringInputStream(private val s: ByteString) : InputStream() {
    private var pos: Int = 0

    override fun available(): Int {
        return s.size() - pos
    }

    override fun skip(n: Long): Long {
        val toSkip = Math.min((s.size() - pos).toLong(), Math.max(n, 0L)).toInt()
        pos += toSkip
        return toSkip.toLong()
    }
    // Oh come on, these are hardly magic...
    @Suppress("MagicNumber")
    override fun read(): Int {
        if (pos >= s.size()) {
            return -1
        }
        val result = s.byteAt(pos).toInt() and 0xff
        pos += 1
        return result
    }

    override fun read(bytes: ByteArray, off: Int, len: Int): Int {
        if (pos >= s.size()) {
            return -1
        }
        val toRead = Math.min(len, s.size() - pos)
        s.copyTo(bytes, pos, off, toRead)
        return toRead
    }
}
