package com.xyswitch.credentials

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import java.io.File
import java.security.KeyStore
import java.security.MessageDigest
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

internal class AndroidCredentialStore(
    context: Context,
    private val alias: String = "com.xyswitch.credentials.aes-gcm.v1",
    directoryName: String = "xy-credentials-v1"
) {
    internal val directory = File(context.noBackupFilesDir, directoryName)
    private val lock = globalLock
    private fun record(target: String): AtomicFile {
        require(target.matches(Regex("XY Switch/(provider|mcp|internal)/[A-Za-z0-9_.-]+")))
        val digest = MessageDigest.getInstance("SHA-256").digest(target.toByteArray(Charsets.UTF_8))
        return AtomicFile(File(directory, digest.joinToString("") { "%02x".format(it) } + ".bin"))
    }
    private fun key(create: Boolean): SecretKey {
        val keystore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        if (keystore.containsAlias(alias)) return (keystore.getEntry(alias, null) as KeyStore.SecretKeyEntry).secretKey
        check(create) { "Keystore key is missing; encrypted credentials cannot be decrypted" }
        // Losing a Keystore key must not silently replace it while old records remain.
        check(directory.listFiles()?.none { it.extension == "bin" || it.extension == "bak" } != false)
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setKeySize(256).setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).setRandomizedEncryptionRequired(true).build())
        }.generateKey()
    }
    fun set(target: String, value: String) = synchronized(lock) {
        val file = record(target)
        val plain = value.toByteArray(Charsets.UTF_8)
        try {
            require(plain.isNotEmpty() && plain.size <= 2560)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, key(true))
            cipher.updateAAD(target.toByteArray(Charsets.UTF_8))
            check(cipher.iv.size == 12)
            val payload = byteArrayOf(1) + cipher.iv + cipher.doFinal(plain)
            check(directory.isDirectory || directory.mkdirs())
            val stream = file.startWrite()
            try { stream.write(payload); file.finishWrite(stream) }
            catch (error: Exception) { file.failWrite(stream); throw error }
        } finally { plain.fill(0) }
    }
    fun get(target: String): String? = synchronized(lock) {
        val file = record(target)
        if (!file.baseFile.exists() && !File(file.baseFile.path + ".bak").exists()) return@synchronized null
        val payload = file.openRead().use { stream ->
            val bounded = ByteArray(4097)
            var total = 0
            while (total < bounded.size) { val n = stream.read(bounded, total, bounded.size-total); if (n < 0) break; total += n }
            require(total in 29..4096)
            bounded.copyOf(total)
        }
        require(payload[0] == 1.toByte())
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(false), GCMParameterSpec(128, payload.copyOfRange(1,13)))
        cipher.updateAAD(target.toByteArray(Charsets.UTF_8))
        val plain = cipher.doFinal(payload, 13, payload.size-13)
        try { String(plain, Charsets.UTF_8) } finally { plain.fill(0) }
    }
    fun delete(target: String) = synchronized(lock) { record(target).delete() }
    companion object { private val globalLock = Any() }
}
