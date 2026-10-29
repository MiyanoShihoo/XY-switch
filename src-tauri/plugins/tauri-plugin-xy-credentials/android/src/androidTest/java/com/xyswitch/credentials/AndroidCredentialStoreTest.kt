package com.xyswitch.credentials

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.security.KeyStore
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class AndroidCredentialStoreTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val suffix = UUID.randomUUID().toString()
    private val alias = "xy-switch-test-$suffix"
    private val name = "xy-credentials-test-$suffix"
    private lateinit var store: AndroidCredentialStore
    private val target = "XY Switch/provider/fixture"
    @Before fun setup() { store = AndroidCredentialStore(context, alias, name) }
    @After fun cleanup() {
        store.directory.deleteRecursively()
        KeyStore.getInstance("AndroidKeyStore").apply { load(null); if (containsAlias(alias)) deleteEntry(alias) }
    }
    @Test fun roundTripSurvivesStoreRecreationAndDeletionIsIdempotent() {
        assertNull(store.get(target))
        store.set(target,"fixture-secret-你好")
        assertEquals("fixture-secret-你好",AndroidCredentialStore(context,alias,name).get(target))
        store.set(target,"replacement")
        assertEquals("replacement",store.get(target))
        store.delete(target); store.delete(target); assertNull(store.get(target))
    }
    @Test fun recordsAreEncryptedAndTamperingFailsClosed() {
        store.set(target,"fixture-sensitive-value")
        val file = store.directory.listFiles()!!.single()
        val bytes = file.readBytes()
        assertFalse(String(bytes,Charsets.ISO_8859_1).contains("fixture-sensitive-value"))
        bytes[bytes.lastIndex] = (bytes.last().toInt() xor 1).toByte(); file.writeBytes(bytes)
        expectFailure { store.get(target) }
        assertTrue(file.exists())
    }
    @Test fun validatesScopesSizesAndBindsCiphertextToItsTarget() {
        expectFailure { store.set("../escape","fixture") }
        expectFailure { store.set(target,"") }
        expectFailure { store.set(target,"x".repeat(2561)) }
        store.set(target,"provider-secret")
        val first = store.directory.listFiles()!!.single().readBytes()
        val other="XY Switch/mcp/fixture"
        store.set(other,"mcp-secret")
        assertEquals("provider-secret",store.get(target)); assertEquals("mcp-secret",store.get(other))
        store.directory.listFiles()!!.filter { !it.readBytes().contentEquals(first) }.single().writeBytes(first)
        expectFailure { store.get(other) }
    }
    @Test fun missingKeystoreKeyDoesNotResetEncryptedRecords() {
        store.set(target,"fixture")
        KeyStore.getInstance("AndroidKeyStore").apply { load(null); deleteEntry(alias) }
        expectFailure { store.get(target) }
        expectFailure { store.set(target,"replacement") }
        assertEquals(1,store.directory.listFiles()!!.size)
    }
    private fun expectFailure(block: () -> Any?) {
        var failed=false
        try { block() } catch (_: Exception) { failed=true }
        assertTrue("Expected failure without resetting data",failed)
    }
}
