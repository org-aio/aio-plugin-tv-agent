package site.addzero.tvagent;

import android.content.Context;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;
import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

final class SecretStore {
    private static final String PREFERENCES = "tv_agent_secrets";
    private static final String PREF_SECRET = "secret";
    private static final String PREF_IV = "iv";
    private static final String ALIAS = "tv-agent-model-key";

    private SecretStore() {
    }

    static String load(Context context) {
        String encoded = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getString(PREF_SECRET, "");
        String iv = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getString(PREF_IV, "");
        if (encoded == null || encoded.isEmpty() || iv == null || iv.isEmpty()) return "";
        try {
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(
                Cipher.DECRYPT_MODE,
                key(),
                new GCMParameterSpec(128, Base64.decode(iv, Base64.NO_WRAP))
            );
            byte[] plain = cipher.doFinal(Base64.decode(encoded, Base64.NO_WRAP));
            return new String(plain, StandardCharsets.UTF_8);
        } catch (Exception ignored) {
            clear(context);
            return "";
        }
    }

    static void save(Context context, String secret) throws Exception {
        if (secret == null || secret.isEmpty()) {
            clear(context);
            return;
        }
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.ENCRYPT_MODE, key());
        byte[] encrypted = cipher.doFinal(secret.getBytes(StandardCharsets.UTF_8));
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .putString(PREF_SECRET, Base64.encodeToString(encrypted, Base64.NO_WRAP))
            .putString(PREF_IV, Base64.encodeToString(cipher.getIV(), Base64.NO_WRAP))
            .apply();
    }

    static void clear(Context context) {
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().clear().apply();
    }

    private static SecretKey key() throws Exception {
        KeyStore store = KeyStore.getInstance("AndroidKeyStore");
        store.load(null);
        if (store.containsAlias(ALIAS)) {
            return ((KeyStore.SecretKeyEntry) store.getEntry(ALIAS, null)).getSecretKey();
        }
        KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        generator.init(new KeyGenParameterSpec.Builder(
            ALIAS,
            KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .build());
        return generator.generateKey();
    }
}
