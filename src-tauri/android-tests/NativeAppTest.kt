package com.xyswitch.desktop

import android.content.Intent
import android.webkit.WebView
import android.view.View
import android.view.ViewGroup
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Runs against the actual release APK with its real Rust IPC and Keystore plugin. */
@RunWith(AndroidJUnit4::class)
class NativeAppTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private fun webView(view: View): WebView? {
        if (view is WebView) return view
        if (view is ViewGroup) for (n in 0 until view.childCount) webView(view.getChildAt(n))?.let { return it }
        return null
    }
    private fun evaluate(scenario: ActivityScenario<MainActivity>, script: String): String {
        val latch = CountDownLatch(1)
        var result: String? = null
        scenario.onActivity { activity ->
            val web = webView(activity.window.decorView)
            assertNotNull("Release application WebView is missing", web)
            web!!.evaluateJavascript(script) { value -> result = value; latch.countDown() }
        }
        assertTrue("WebView evaluation timed out", latch.await(15, TimeUnit.SECONDS))
        return result ?: "null"
    }
    private fun waitForUi(scenario: ActivityScenario<MainActivity>) {
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(90)
        while (System.nanoTime() < end) {
            // ActivityScenario can resume before Tauri attaches its asynchronously created WebView.
            var attached = false
            scenario.onActivity { activity -> attached = webView(activity.window.decorView) != null }
            if (attached && evaluate(scenario, "!!(window.__TAURI__ && window.__TAURI__.core && document.getElementById('root') && document.getElementById('root').children.length)") == "true") return
            Thread.sleep(500)
        }
        fail("Release frontend and Tauri IPC did not become ready")
    }
    private fun invoke(scenario: ActivityScenario<MainActivity>, command: String, arguments: JSONObject = JSONObject()): JSONObject {
        val marker = "xyNativeTest" + UUID.randomUUID().toString().replace("-", "")
        val script = "window['$marker']=null;window.__TAURI__.core.invoke(${JSONObject.quote(command)},$arguments).then(value=>{window['$marker']={ok:true,value};}).catch(error=>{window['$marker']={ok:false,error:String(error)};});'started'"
        evaluate(scenario, script)
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(30)
        while (System.nanoTime() < end) {
            val json = evaluate(scenario, "window['$marker']")
            if (json != "null") {
                evaluate(scenario, "delete window['$marker']")
                return JSONObject(json)
            }
            Thread.sleep(100)
        }
        fail("Rust IPC timed out for $command (possible Android main-thread deadlock)")
        return JSONObject()
    }
    private fun ok(scenario: ActivityScenario<MainActivity>, command: String, arguments: JSONObject = JSONObject()): JSONObject {
        val result = invoke(scenario, command, arguments)
        assertTrue("$command failed: ${result.optString("error")}", result.getBoolean("ok"))
        return result
    }
    private fun launch(): ActivityScenario<MainActivity> {
        val intent = Intent(instrumentation.targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        return ActivityScenario.launch<MainActivity>(intent)
    }
    @Test fun releaseAppUsesKeystoreThroughRustAndRejectsPrivateJavaScriptBridge() {
        val fixture = "android-ci-" + UUID.randomUUID().toString()
        val original = "fixture-secret-" + UUID.randomUUID().toString()
        val replacement = "fixture-replacement-" + UUID.randomUUID().toString()
        launch().use { scenario ->
            waitForUi(scenario)
            val platform = ok(scenario, "app_platform").getJSONObject("value")
            assertEquals("android", platform.getString("os"))
            assertEquals("Android Keystore", platform.getString("credentialStore"))
            assertTrue(platform.getBoolean("mobile")); assertFalse(platform.getBoolean("clientTakeover"))
            assertFalse(platform.getBoolean("systemTray")); assertFalse(platform.getBoolean("autostart"))
            ok(scenario, "app_paths") // Also exercises Android's path plugin off the UI thread.
            val args = JSONObject().put("providerId", fixture)
            try {
                assertFalse(ok(scenario, "has_provider_api_key", args).getBoolean("value"))
                ok(scenario, "set_provider_api_key", JSONObject(args.toString()).put("apiKey", original))
                assertTrue(ok(scenario, "has_provider_api_key", args).getBoolean("value"))
                for (operation in listOf("get", "set", "delete")) {
                    val privateArgs = JSONObject().put("operation", operation).put("target", "XY Switch/provider/$fixture").put("value", "attacker-fixture")
                    val denied = invoke(scenario, "plugin:xy-credentials|access", privateArgs)
                    assertFalse("The private credential bridge was exposed to JavaScript", denied.getBoolean("ok"))
                    assertFalse(denied.toString().contains(original))
                }
                // A local HTTP fixture verifies Rust read/decrypt, not just existence of ciphertext.
                val server = java.net.ServerSocket(0, 1, java.net.InetAddress.getByName("127.0.0.1"))
                server.soTimeout = 30000
                var tokenMatched = false
                var serverFailure: Throwable? = null
                val thread = Thread {
                    try {
                        server.accept().use { socket ->
                            socket.soTimeout = 30000
                            val reader = socket.getInputStream().bufferedReader(Charsets.US_ASCII)
                            var line: String?
                            do {
                                line = reader.readLine()
                                if (line == "Authorization: Bearer $original" || line?.lowercase() == "authorization: bearer ${original.lowercase()}") tokenMatched = true
                            } while (!line.isNullOrEmpty())
                            val body = "{\"data\":[{\"id\":\"fixture-model\"}]}"
                            socket.getOutputStream().write(("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ${body.toByteArray().size}\r\nConnection: close\r\n\r\n$body").toByteArray())
                        }
                    } catch (error: Throwable) { serverFailure = error }
                }
                thread.start()
                try {
                    val request = JSONObject().put("providerId", fixture).put("providerType", "openai").put("upstreamFormat", "openai_chat").put("endpoint", "http://127.0.0.1:${server.localPort}")
                    val models = ok(scenario, "discover_provider_models", JSONObject().put("request", request)).getJSONArray("value")
                    assertEquals("fixture-model", models.getJSONObject(0).getString("id"))
                    thread.join(35000)
                    assertFalse("Local fixture HTTP server did not finish", thread.isAlive)
                    assertNull(serverFailure)
                    assertTrue("Rust did not read the original secret from Android Keystore", tokenMatched)
                } finally { server.close(); thread.join(1000) }
                ok(scenario, "set_provider_api_key", JSONObject(args.toString()).put("apiKey", replacement))
                scenario.recreate(); waitForUi(scenario)
                assertTrue(ok(scenario, "has_provider_api_key", args).getBoolean("value"))
            } finally {
                ok(scenario, "delete_provider_api_key", args)
                ok(scenario, "delete_provider_api_key", args)
                assertFalse(ok(scenario, "has_provider_api_key", args).getBoolean("value"))
            }
        }
    }
}
