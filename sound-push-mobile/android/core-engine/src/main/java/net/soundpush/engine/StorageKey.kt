package net.soundpush.engine

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * 32-byte key protecting the engine's identity and trust store.
 * Stored encrypted with an AES-GCM key that never leaves the Android Keystore.
 */
object StorageKey {
    private const val ALIAS = "soundpush-storage"
    private const val FILE = "storage.key.enc"

    fun get(context: Context): ByteArray {
        val file = File(context.noBackupFilesDir, FILE)
        val wrapping = wrappingKey()
        if (file.exists()) {
            runCatching {
                val bytes = file.readBytes()
                val iv = bytes.copyOfRange(0, 12)
                val cipher = Cipher.getInstance("AES/GCM/NoPadding")
                cipher.init(Cipher.DECRYPT_MODE, wrapping, GCMParameterSpec(128, iv))
                return cipher.doFinal(bytes.copyOfRange(12, bytes.size))
            }
        }
        val key = ByteArray(32).also { SecureRandom().nextBytes(it) }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, wrapping)
        file.writeBytes(cipher.iv + cipher.doFinal(key))
        return key
    }

    private fun wrappingKey(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }
}
