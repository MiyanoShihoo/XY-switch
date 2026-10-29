package com.xyswitch.credentials

import android.app.Activity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject

@InvokeArg
class CredentialArgs {
    var operation: String = ""
    var target: String = ""
    var value: String? = null
}

@TauriPlugin
class CredentialPlugin(activity: Activity) : Plugin(activity) {
    private val store = AndroidCredentialStore(activity.applicationContext)
    @Command
    fun access(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CredentialArgs::class.java)
            val value = when (args.operation) {
                "set" -> { store.set(args.target, requireNotNull(args.value)); null }
                "get" -> store.get(args.target)
                "delete" -> { store.delete(args.target); null }
                else -> throw IllegalArgumentException("Unsupported credential operation")
            }
            val result = JSObject()
            result.put("value", value ?: JSONObject.NULL)
            invoke.resolve(result)
        } catch (error: Exception) {
            // Do not log arguments, ciphertext, platform exception messages, or secrets.
            invoke.reject("Secure credential operation failed (${error.javaClass.simpleName}); existing data was not reset")
        }
    }
}
