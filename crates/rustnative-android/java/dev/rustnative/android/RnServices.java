package dev.rustnative.android;

import android.Manifest;
import android.app.Activity;
import android.app.Application;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.net.ConnectivityManager;
import android.net.NetworkCapabilities;
import android.net.Uri;
import android.os.BatteryManager;
import android.os.Build;
import android.os.Bundle;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyInfo;
import android.security.keystore.KeyProperties;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.security.KeyStore;
import java.security.MessageDigest;
import java.security.cert.Certificate;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.SecretKeyFactory;
import javax.crypto.spec.GCMParameterSpec;

/**
 * The Java half of the portable services (`services/*`). Rust calls these
 * on whatever thread the service runs on; the ones that need an activity
 * post to the main thread and answer later — an activity result or a
 * permission answer — through {@link RnBridge}.
 *
 * <p>Errors come back as a message (a {@code String} result that is not
 * null), never as an exception crossing into Rust.
 */
final class RnServices {
    private RnServices() {}

    /** Request codes this class starts activities with: the base plus a token. */
    static final int REQUEST_BASE = 0x5200;

    private static Context context;
    /** The activity in front, for what needs one (file dialogs, permissions). */
    private static Activity top;

    /** Records the application context and follows the activity in front. */
    static synchronized void init(Context application) {
        if (context != null) {
            return;
        }
        context = application.getApplicationContext();
        ((Application) context).registerActivityLifecycleCallbacks(
            new Application.ActivityLifecycleCallbacks() {
                @Override public void onActivityCreated(Activity activity, Bundle saved) { top = activity; }
                @Override public void onActivityStarted(Activity activity) { top = activity; }
                @Override public void onActivityResumed(Activity activity) { top = activity; }
                @Override public void onActivityPaused(Activity activity) {}
                @Override public void onActivityStopped(Activity activity) {}
                @Override public void onActivitySaveInstanceState(Activity activity, Bundle out) {}
                @Override public void onActivityDestroyed(Activity activity) {
                    if (top == activity) {
                        top = null;
                    }
                }
            });
    }

    static Context context() {
        return context;
    }

    static String packageName() {
        return context.getPackageName();
    }

    static String filesDir() {
        return context.getFilesDir().getAbsolutePath();
    }

    static String cacheDir() {
        return context.getCacheDir().getAbsolutePath();
    }

    // ---- Clipboard. ----

    static String readClipboard() {
        ClipboardManager clipboard = context.getSystemService(ClipboardManager.class);
        if (clipboard == null || !clipboard.hasPrimaryClip()) {
            return null;
        }
        ClipData clip = clipboard.getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) {
            return null;
        }
        CharSequence text = clip.getItemAt(0).coerceToText(context);
        return text == null ? null : text.toString();
    }

    static void writeClipboard(String text) {
        ClipboardManager clipboard = context.getSystemService(ClipboardManager.class);
        if (clipboard != null) {
            clipboard.setPrimaryClip(ClipData.newPlainText("text", text));
        }
    }

    // ---- Intents: URLs and sharing. ----

    /** Opens {@code url} in whatever handles it; an error message, or null. */
    static String openUrl(String url) {
        Intent intent = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            context.startActivity(intent);
            return null;
        } catch (android.content.ActivityNotFoundException e) {
            return "nothing on this device opens " + url;
        }
    }

    /** Offers {@code text} to the share sheet; an error message, or null. */
    static String share(String text, String subject) {
        Intent send = new Intent(Intent.ACTION_SEND);
        send.setType("text/plain");
        send.putExtra(Intent.EXTRA_TEXT, text);
        if (subject != null) {
            send.putExtra(Intent.EXTRA_SUBJECT, subject);
        }
        Intent chooser = Intent.createChooser(send, null);
        chooser.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            context.startActivity(chooser);
            return null;
        } catch (android.content.ActivityNotFoundException e) {
            return "this device has no share sheet";
        }
    }

    // ---- Notifications. ----

    /** The channel notifications post on unless they name another. */
    static final String DEFAULT_CHANNEL = "rustnative.default";

    /**
     * Posts a notification on {@code channel} (created on first use), with
     * {@code actions} as (id, label) pairs whose taps return to the
     * application as surface actions. An error message, or null.
     */
    static String notify(int id, String channel, String title, String body, String[] actions) {
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        if (manager == null) {
            return "no notification manager";
        }
        if (Build.VERSION.SDK_INT >= 33
            && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                != PackageManager.PERMISSION_GRANTED) {
            return "POST_NOTIFICATIONS is not granted: ask for Permission::Notifications first";
        }
        if (!manager.areNotificationsEnabled()) {
            return "notifications are turned off for this application";
        }
        String name = channel == null ? DEFAULT_CHANNEL : channel;
        if (manager.getNotificationChannel(name) == null) {
            manager.createNotificationChannel(new NotificationChannel(name,
                name.equals(DEFAULT_CHANNEL) ? "Notifications" : name,
                NotificationManager.IMPORTANCE_DEFAULT));
        }
        android.app.Notification.Builder builder = new android.app.Notification.Builder(context, name)
            .setSmallIcon(icon())
            .setContentTitle(title)
            .setContentText(body)
            .setAutoCancel(true)
            .setContentIntent(surfaceIntent("notification", "", id * 16));
        for (int i = 0; actions != null && i + 1 < actions.length; i += 2) {
            builder.addAction(new android.app.Notification.Action.Builder(null, actions[i + 1],
                surfaceIntent("notification", actions[i], id * 16 + 1 + i / 2)).build());
        }
        manager.notify(id, builder.build());
        return null;
    }

    /** Ongoing: a notification that stays, with progress (−1: indeterminate). */
    static String ongoing(int id, String title, String body, int progress, String[] actions) {
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        if (manager == null) {
            return "no notification manager";
        }
        if (manager.getNotificationChannel(DEFAULT_CHANNEL) == null) {
            manager.createNotificationChannel(new NotificationChannel(DEFAULT_CHANNEL,
                "Notifications", NotificationManager.IMPORTANCE_DEFAULT));
        }
        android.app.Notification.Builder builder = new android.app.Notification.Builder(context, DEFAULT_CHANNEL)
            .setSmallIcon(icon())
            .setContentTitle(title)
            .setContentText(body)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setProgress(100, Math.max(0, progress), progress < 0)
            .setContentIntent(surfaceIntent("ongoing", "", id * 16));
        for (int i = 0; actions != null && i + 1 < actions.length; i += 2) {
            builder.addAction(new android.app.Notification.Action.Builder(null, actions[i + 1],
                surfaceIntent("ongoing", actions[i], id * 16 + 1 + i / 2)).build());
        }
        try {
            manager.notify(id, builder.build());
            return null;
        } catch (SecurityException e) {
            return "POST_NOTIFICATIONS is not granted";
        }
    }

    static boolean channelExists(String name) {
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        return manager != null && manager.getNotificationChannel(name) != null;
    }

    /**
     * Publishes the launcher's shortcuts (a long press on the icon): each
     * opens the application with its arguments as a surface action.
     */
    static String setShortcuts(String[] labels, String[] arguments) {
        android.content.pm.ShortcutManager manager = context.getSystemService(android.content.pm.ShortcutManager.class);
        if (manager == null) {
            return "this launcher has no shortcuts";
        }
        List<android.content.pm.ShortcutInfo> shortcuts = new ArrayList<>();
        int count = Math.min(labels.length, manager.getMaxShortcutCountPerActivity());
        for (int i = 0; i < count; i++) {
            Intent intent = context.getPackageManager().getLaunchIntentForPackage(context.getPackageName());
            if (intent == null) {
                return "this application has no launcher activity";
            }
            intent.setAction(Intent.ACTION_VIEW);
            intent.putExtra(RnIntents.EXTRA_SURFACE, "shortcut");
            intent.putExtra(RnIntents.EXTRA_ACTION, arguments[i]);
            shortcuts.add(new android.content.pm.ShortcutInfo.Builder(context, "rustnative." + i)
                .setShortLabel(labels[i])
                .setRank(i)
                .setIntent(intent)
                .build());
        }
        try {
            manager.setDynamicShortcuts(shortcuts);
            return null;
        } catch (IllegalStateException e) {
            return "the launcher is rate-limiting shortcut updates";
        }
    }

    /** The published shortcuts' labels (tests read them). */
    static String[] shortcuts() {
        android.content.pm.ShortcutManager manager = context.getSystemService(android.content.pm.ShortcutManager.class);
        List<String> labels = new ArrayList<>();
        if (manager != null) {
            List<android.content.pm.ShortcutInfo> published = new ArrayList<>(manager.getDynamicShortcuts());
            published.sort((a, b) -> Integer.compare(a.getRank(), b.getRank()));
            for (android.content.pm.ShortcutInfo shortcut : published) {
                labels.add(String.valueOf(shortcut.getShortLabel()));
            }
        }
        return labels.toArray(new String[0]);
    }

    // ---- Updates (sideloaded installations only; `update.rs`). ----

    /** The package that installed this application (null: adb or unknown). */
    static String installer() {
        try {
            if (Build.VERSION.SDK_INT >= 30) {
                return context.getPackageManager()
                    .getInstallSourceInfo(context.getPackageName()).getInstallingPackageName();
            }
            return context.getPackageManager().getInstallerPackageName(context.getPackageName());
        } catch (Exception e) {
            return null;
        }
    }

    /**
     * Hands an APK to the system package installer, which asks the person
     * to confirm; an error message, or null once the session is committed.
     */
    static String installPackage(byte[] apk) {
        try {
            android.content.pm.PackageInstaller installer = context.getPackageManager().getPackageInstaller();
            android.content.pm.PackageInstaller.SessionParams params =
                new android.content.pm.PackageInstaller.SessionParams(
                    android.content.pm.PackageInstaller.SessionParams.MODE_FULL_INSTALL);
            params.setAppPackageName(context.getPackageName());
            int id = installer.createSession(params);
            try (android.content.pm.PackageInstaller.Session session = installer.openSession(id)) {
                try (OutputStream out = session.openWrite("update.apk", 0, apk.length)) {
                    out.write(apk);
                    session.fsync(out);
                }
                Intent status = new Intent(context, RnActivity.class)
                    .setAction("dev.rustnative.UPDATE_STATUS");
                session.commit(PendingIntent.getActivity(context, id, status,
                    PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE).getIntentSender());
            }
            return null;
        } catch (Exception e) {
            return e.toString();
        }
    }

    // ---- Widgets and tiles (their components keep what Rust sends). ----

    static boolean updateWidget(String id, String title, String[] lines, String[] actions) {
        return RnWidgetProvider.update(context, id, title, lines, actions);
    }

    static String widgetContent(String id) {
        return RnWidgetProvider.content(context, id);
    }

    static boolean updateTile(String id, String label, String subtitle, boolean active) {
        return RnTileService.update(context, id, label, subtitle, active);
    }

    static String[] tileState(String id) {
        return RnTileService.state(context, id);
    }

    static void cancelNotification(int id) {
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        if (manager != null) {
            manager.cancel(id);
        }
    }

    /** Whether notification {@code id} is showing (tests read it back). */
    static boolean notificationShowing(int id) {
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        if (manager == null) {
            return false;
        }
        for (android.service.notification.StatusBarNotification shown : manager.getActiveNotifications()) {
            if (shown.getId() == id) {
                return true;
            }
        }
        return false;
    }

    private static int icon() {
        int icon = context.getApplicationInfo().icon;
        return icon != 0 ? icon : android.R.drawable.ic_dialog_info;
    }

    /** An intent that opens the launcher activity carrying a surface's action. */
    static PendingIntent surfaceIntent(String surface, String action, int request) {
        Intent intent = context.getPackageManager().getLaunchIntentForPackage(context.getPackageName());
        if (intent == null) {
            intent = new Intent(context, RnActivity.class);
        }
        intent.setFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_SINGLE_TOP);
        intent.putExtra(RnIntents.EXTRA_SURFACE, surface);
        intent.putExtra(RnIntents.EXTRA_ACTION, action);
        return PendingIntent.getActivity(context, request, intent,
            PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
    }

    // ---- File dialogs: the Storage Access Framework. ----

    /**
     * Starts the system picker: kind 0 opens a file, 1 creates one, 2 picks
     * a folder. The answer arrives as the activity result for
     * {@code REQUEST_BASE + token}. An error message, or null.
     */
    static String pickDocument(final int token, final int kind, final String[] extensions, final String title) {
        List<String> types = new ArrayList<>();
        for (String extension : extensions) {
            String type = android.webkit.MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension);
            if (type != null && !types.contains(type)) {
                types.add(type);
            }
        }
        final String[] mimeTypes = types.toArray(new String[0]);
        final Activity activity = top;
        if (activity == null) {
            return "no activity is in front to show the picker over";
        }
        final Intent intent;
        if (kind == 2) {
            intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        } else {
            intent = new Intent(kind == 1 ? Intent.ACTION_CREATE_DOCUMENT : Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType(mimeTypes.length == 1 ? mimeTypes[0] : kind == 1 ? "application/octet-stream" : "*/*");
            if (mimeTypes.length > 1) {
                intent.putExtra(Intent.EXTRA_MIME_TYPES, mimeTypes);
            }
            if (kind == 1 && title != null) {
                intent.putExtra(Intent.EXTRA_TITLE, title);
            }
        }
        RnBridge.main().post(new Runnable() {
            @Override
            public void run() {
                try {
                    activity.startActivityForResult(intent, REQUEST_BASE + token);
                } catch (android.content.ActivityNotFoundException e) {
                    RnBridge.nativeActivityResult(REQUEST_BASE + token, Activity.RESULT_CANCELED, null);
                }
            }
        });
        return null;
    }

    /** The document an activity result chose (its content URI), or null. */
    static String resultUri(Intent data) {
        if (data == null || data.getData() == null) {
            return null;
        }
        Uri uri = data.getData();
        int flags = Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION;
        try {
            context.getContentResolver().takePersistableUriPermission(uri, flags & data.getFlags());
        } catch (SecurityException ignored) {
            // Not persistable: the grant lasts while the application runs.
        }
        return uri.toString();
    }

    /** A content URI's bytes, or null. */
    static byte[] readUri(String uri) {
        try (InputStream in = context.getContentResolver().openInputStream(Uri.parse(uri))) {
            return in == null ? null : drain(in);
        } catch (Exception e) {
            return null;
        }
    }

    /** Writes a content URI; an error message, or null. */
    static String writeUri(String uri, byte[] bytes) {
        try (OutputStream out = context.getContentResolver().openOutputStream(Uri.parse(uri), "wt")) {
            if (out == null) {
                return "the provider refused to open " + uri;
            }
            out.write(bytes);
            return null;
        } catch (Exception e) {
            return e.toString();
        }
    }

    // ---- Permissions. ----

    // The portable states (`PermissionState`), as numbers.
    static final int NOT_ASKED = 0;
    static final int GRANTED = 1;
    static final int LIMITED = 2;
    static final int DENIED = 3;
    static final int PERMANENTLY_DENIED = 4;

    /** The Android permissions behind a portable one (`Permission` as a number). */
    static String[] permissionsFor(int permission) {
        switch (permission) {
            case 0: return new String[] {Manifest.permission.CAMERA};
            case 1: return new String[] {Manifest.permission.RECORD_AUDIO};
            case 2: return new String[] {Manifest.permission.ACCESS_FINE_LOCATION,
                Manifest.permission.ACCESS_COARSE_LOCATION};
            case 3: return Build.VERSION.SDK_INT >= 33
                ? new String[] {Manifest.permission.POST_NOTIFICATIONS} : new String[0];
            case 4: return new String[] {Manifest.permission.READ_CONTACTS};
            case 5:
                if (Build.VERSION.SDK_INT >= 34) {
                    return new String[] {Manifest.permission.READ_MEDIA_IMAGES,
                        Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED};
                }
                return new String[] {Build.VERSION.SDK_INT >= 33
                    ? Manifest.permission.READ_MEDIA_IMAGES : Manifest.permission.READ_EXTERNAL_STORAGE};
            case 6: return Build.VERSION.SDK_INT >= 31
                ? new String[] {Manifest.permission.BLUETOOTH_CONNECT} : new String[0];
            default: return new String[0];
        }
    }

    private static SharedPreferences asked() {
        return context.getSharedPreferences("rustnative.permissions", Context.MODE_PRIVATE);
    }

    private static boolean granted(String permission) {
        return context.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED;
    }

    /** Where a portable permission stands. */
    static int permissionState(int permission) {
        String[] names = permissionsFor(permission);
        if (names.length == 0) {
            // An install-time permission on this API level.
            if (permission == 3) {
                NotificationManager manager = context.getSystemService(NotificationManager.class);
                return manager != null && manager.areNotificationsEnabled() ? GRANTED : PERMANENTLY_DENIED;
            }
            return GRANTED;
        }
        if (granted(names[0])) {
            return GRANTED;
        }
        // Part of it: approximate location, or the photos the person chose.
        if (names.length > 1 && granted(names[1])) {
            return LIMITED;
        }
        if (!asked().getBoolean(names[0], false)) {
            return NOT_ASKED;
        }
        Activity activity = top;
        if (activity != null && activity.shouldShowRequestPermissionRationale(names[0])) {
            return DENIED;
        }
        // Asked, refused, and Android will not show the prompt again
        // ("don't ask again", twice refused, or policy).
        return PERMANENTLY_DENIED;
    }

    /**
     * Asks for a portable permission; the answer arrives as the permissions
     * result for {@code REQUEST_BASE + token}. False when there is nothing
     * to ask (no activity, or an install-time permission).
     */
    static boolean requestPermission(final int token, int permission) {
        final String[] names = permissionsFor(permission);
        final Activity activity = top;
        if (names.length == 0 || activity == null) {
            return false;
        }
        SharedPreferences.Editor editor = asked().edit();
        for (String name : names) {
            editor.putBoolean(name, true);
        }
        editor.apply();
        RnBridge.main().post(new Runnable() {
            @Override
            public void run() {
                activity.requestPermissions(names, REQUEST_BASE + token);
            }
        });
        return true;
    }

    /** Opens this application's settings page. */
    static boolean openSettings() {
        Intent intent = new Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
            Uri.fromParts("package", context.getPackageName(), null));
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            context.startActivity(intent);
            return true;
        } catch (android.content.ActivityNotFoundException e) {
            return false;
        }
    }

    // ---- Secure storage: an AES-GCM key in the Android Keystore. ----

    private static final String KEY_ALIAS = "rustnative.secure-storage";

    private static SecretKey key() throws Exception {
        KeyStore store = KeyStore.getInstance("AndroidKeyStore");
        store.load(null);
        if (store.containsAlias(KEY_ALIAS)) {
            return ((KeyStore.SecretKeyEntry) store.getEntry(KEY_ALIAS, null)).getSecretKey();
        }
        KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        KeyGenParameterSpec.Builder spec = new KeyGenParameterSpec.Builder(KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256);
        if (Build.VERSION.SDK_INT >= 28
            && context.getPackageManager().hasSystemFeature(PackageManager.FEATURE_STRONGBOX_KEYSTORE)) {
            try {
                generator.init(spec.setIsStrongBoxBacked(true).build());
                return generator.generateKey();
            } catch (Exception strongBoxRefused) {
                spec.setIsStrongBoxBacked(false);
            }
        }
        generator.init(spec.build());
        return generator.generateKey();
    }

    /** Whether the key lives in secure hardware (a TEE or StrongBox). */
    static boolean secureHardware() {
        try {
            SecretKey key = key();
            KeyInfo info = (KeyInfo) SecretKeyFactory.getInstance(key.getAlgorithm(), "AndroidKeyStore")
                .getKeySpec(key, KeyInfo.class);
            if (Build.VERSION.SDK_INT >= 31) {
                int level = info.getSecurityLevel();
                return level == KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT
                    || level == KeyProperties.SECURITY_LEVEL_STRONGBOX;
            }
            return info.isInsideSecureHardware();
        } catch (Exception e) {
            return false;
        }
    }

    private static File secret(String name) {
        File directory = new File(context.getFilesDir(), "rustnative/secrets");
        directory.mkdirs();
        StringBuilder hex = new StringBuilder();
        for (byte b : name.getBytes(java.nio.charset.StandardCharsets.UTF_8)) {
            hex.append(String.format("%02x", b));
        }
        return new File(directory, hex.toString());
    }

    /** Seals {@code value} under {@code name}; an error message, or null. */
    static String sealSecret(String name, byte[] value) {
        try {
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.ENCRYPT_MODE, key());
            byte[] iv = cipher.getIV();
            byte[] sealed = cipher.doFinal(value);
            File file = secret(name);
            File temporary = new File(file.getPath() + ".tmp");
            try (FileOutputStream out = new FileOutputStream(temporary)) {
                out.write(iv.length);
                out.write(iv);
                out.write(sealed);
                out.getFD().sync();
            }
            if (!temporary.renameTo(file)) {
                return "could not replace " + file;
            }
            return null;
        } catch (Exception e) {
            return e.toString();
        }
    }

    /** The secret under {@code name}, or null when there is none. Throws on a refusal. */
    static byte[] openSecret(String name) throws Exception {
        File file = secret(name);
        if (!file.exists()) {
            return null;
        }
        byte[] stored;
        try (InputStream in = new java.io.FileInputStream(file)) {
            stored = drain(in);
        }
        int length = stored[0];
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.DECRYPT_MODE, key(), new GCMParameterSpec(128, stored, 1, length));
        return cipher.doFinal(stored, 1 + length, stored.length - 1 - length);
    }

    static boolean deleteSecret(String name) {
        File file = secret(name);
        return !file.exists() || file.delete();
    }

    // ---- HTTP: HttpsURLConnection with the system trust store. ----

    /** A request in flight, then its response, read field by field from Rust. */
    static final class Response {
        HttpURLConnection connection;
        byte[] digest;
        int status;
        String[] headers;
        byte[] body;
        String error;
    }

    /**
     * Opens a connection. {@code headers} alternate name and value. When
     * {@code pinned}, it connects now, records the SHA-256 of the peer's
     * leaf certificate ({@link #digest}) for Rust to check before a byte of
     * the request is written, and follows no redirects.
     */
    static Response open(String method, String url, String[] headers, boolean pinned, int timeoutMillis) {
        Response response = new Response();
        try {
            HttpURLConnection connection = (HttpURLConnection) new URL(url).openConnection();
            response.connection = connection;
            connection.setRequestMethod(method);
            connection.setConnectTimeout(timeoutMillis);
            connection.setReadTimeout(timeoutMillis);
            connection.setRequestProperty("User-Agent", "RustNative");
            for (int i = 0; i + 1 < headers.length; i += 2) {
                connection.addRequestProperty(headers[i], headers[i + 1]);
            }
            if (pinned) {
                if (!(connection instanceof javax.net.ssl.HttpsURLConnection)) {
                    response.error = connection.getURL().getHost()
                        + " is pinned, and cannot be reached over plain HTTP";
                    return response;
                }
                connection.setInstanceFollowRedirects(false);
                connection.connect();
                Certificate[] chain = ((javax.net.ssl.HttpsURLConnection) connection).getServerCertificates();
                response.digest = MessageDigest.getInstance("SHA-256").digest(chain[0].getEncoded());
            }
        } catch (Exception e) {
            response.error = "the request to " + url + " failed: " + e;
        }
        return response;
    }

    /** Sends the body (if any) and reads the response; closes the connection. */
    static void exchange(Response response, byte[] body) {
        HttpURLConnection connection = response.connection;
        try {
            String method = connection.getRequestMethod();
            if (body.length > 0 || method.equals("POST") || method.equals("PUT") || method.equals("PATCH")) {
                connection.setDoOutput(true);
                try (OutputStream out = connection.getOutputStream()) {
                    out.write(body);
                }
            }
            response.status = connection.getResponseCode();
            List<String> pairs = new ArrayList<>();
            for (Map.Entry<String, List<String>> entry : connection.getHeaderFields().entrySet()) {
                if (entry.getKey() == null) {
                    continue;
                }
                for (String value : entry.getValue()) {
                    pairs.add(entry.getKey());
                    pairs.add(value);
                }
            }
            response.headers = pairs.toArray(new String[0]);
            InputStream in = response.status >= 400 ? connection.getErrorStream() : connection.getInputStream();
            response.body = in == null ? new byte[0] : drain(in);
        } catch (Exception e) {
            response.error = "the request to " + connection.getURL() + " failed: " + e;
        } finally {
            connection.disconnect();
        }
    }

    /** Abandons a connection (a pin did not match). */
    static void close(Response response) {
        if (response.connection != null) {
            response.connection.disconnect();
        }
    }

    static byte[] digest(Response response) { return response.digest; }
    static int status(Response response) { return response.status; }
    static String[] headers(Response response) { return response.headers == null ? new String[0] : response.headers; }
    static byte[] body(Response response) { return response.body == null ? new byte[0] : response.body; }
    static String error(Response response) { return response.error; }

    // ---- Locale: ICU. ----

    private static android.icu.util.ULocale locale(String tag) {
        return android.icu.util.ULocale.forLanguageTag(tag);
    }

    static String formatNumber(String tag, double value, int decimals) {
        android.icu.text.NumberFormat format = android.icu.text.NumberFormat.getInstance(locale(tag));
        format.setMinimumFractionDigits(decimals);
        format.setMaximumFractionDigits(decimals);
        return format.format(value);
    }

    static String formatCurrency(String tag, double value, String currency) {
        android.icu.text.NumberFormat format = android.icu.text.NumberFormat.getCurrencyInstance(locale(tag));
        format.setCurrency(android.icu.util.Currency.getInstance(currency));
        return format.format(value);
    }

    static String formatDate(String tag, int year, int month, int day, boolean long_) {
        android.icu.util.Calendar calendar = android.icu.util.Calendar.getInstance(
            android.icu.util.TimeZone.GMT_ZONE, locale(tag));
        calendar.clear();
        calendar.set(year, month - 1, day);
        return android.icu.text.DateFormat.getDateInstance(
            long_ ? android.icu.text.DateFormat.LONG : android.icu.text.DateFormat.SHORT, locale(tag))
            .format(calendar.getTime());
    }

    static String formatTime(String tag, int hour, int minute, int second) {
        android.icu.util.Calendar calendar = android.icu.util.Calendar.getInstance(
            android.icu.util.TimeZone.GMT_ZONE, locale(tag));
        calendar.clear();
        calendar.set(1970, 0, 1, hour, minute, second);
        android.icu.text.DateFormat format = android.icu.text.DateFormat.getTimeInstance(
            android.icu.text.DateFormat.SHORT, locale(tag));
        format.setTimeZone(android.icu.util.TimeZone.GMT_ZONE);
        return format.format(calendar.getTime());
    }

    static int compare(String tag, String a, String b) {
        return Integer.signum(android.icu.text.Collator.getInstance(locale(tag)).compare(a, b));
    }

    static String upper(String tag, String text) {
        return android.icu.lang.UCharacter.toUpperCase(locale(tag), text);
    }

    static String lower(String tag, String text) {
        return android.icu.lang.UCharacter.toLowerCase(locale(tag), text);
    }

    // ---- Conditions. ----

    static boolean network() {
        ConnectivityManager manager = context.getSystemService(ConnectivityManager.class);
        if (manager == null) {
            return false;
        }
        NetworkCapabilities capabilities = manager.getNetworkCapabilities(manager.getActiveNetwork());
        return capabilities != null
            && capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET);
    }

    static boolean charging() {
        BatteryManager battery = context.getSystemService(BatteryManager.class);
        return battery != null && battery.isCharging();
    }

    // ---- Images. ----

    /**
     * Decodes {@code bytes}, downscaled to fit {@code width}×{@code height}
     * (0: as is): the width, the height, then ARGB pixels; null when the
     * bytes are not an image the platform decodes.
     */
    static int[] decode(byte[] bytes, int width, int height) {
        BitmapFactory.Options bounds = new BitmapFactory.Options();
        bounds.inJustDecodeBounds = true;
        BitmapFactory.decodeByteArray(bytes, 0, bytes.length, bounds);
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) {
            return null;
        }
        BitmapFactory.Options options = new BitmapFactory.Options();
        options.inPreferredConfig = Bitmap.Config.ARGB_8888;
        int sample = 1;
        while (width > 0 && height > 0
            && bounds.outWidth / (sample * 2) >= width && bounds.outHeight / (sample * 2) >= height) {
            sample *= 2;
        }
        options.inSampleSize = sample;
        Bitmap bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.length, options);
        if (bitmap == null) {
            return null;
        }
        if (width > 0 && height > 0 && (bitmap.getWidth() > width || bitmap.getHeight() > height)) {
            double scale = Math.min((double) width / bitmap.getWidth(), (double) height / bitmap.getHeight());
            bitmap = Bitmap.createScaledBitmap(bitmap,
                Math.max(1, (int) Math.round(bitmap.getWidth() * scale)),
                Math.max(1, (int) Math.round(bitmap.getHeight() * scale)), true);
        }
        int w = bitmap.getWidth();
        int h = bitmap.getHeight();
        int[] out = new int[2 + w * h];
        out[0] = w;
        out[1] = h;
        bitmap.getPixels(out, 2, w, 0, 0, w, h);
        return out;
    }

    // ---- Printing: a PDF, to a file or through the system's print UI. ----

    /** Lays the pages' lines out as a PDF in {@code path}; an error message, or null. */
    static String writePdf(String path, String[] lines, int[] pageStarts) {
        android.graphics.pdf.PdfDocument document = new android.graphics.pdf.PdfDocument();
        android.graphics.Paint paint = new android.graphics.Paint();
        paint.setTextSize(11);
        try {
            for (int page = 0; page < pageStarts.length; page++) {
                int end = page + 1 < pageStarts.length ? pageStarts[page + 1] : lines.length;
                android.graphics.pdf.PdfDocument.Page sheet = document.startPage(
                    new android.graphics.pdf.PdfDocument.PageInfo.Builder(595, 842, page + 1).create());
                float y = 56;
                for (int line = pageStarts[page]; line < end; line++) {
                    sheet.getCanvas().drawText(lines[line], 56, y, paint);
                    y += 15;
                }
                document.finishPage(sheet);
            }
            try (FileOutputStream out = new FileOutputStream(path)) {
                document.writeTo(out);
            }
            return null;
        } catch (Exception e) {
            return e.toString();
        } finally {
            document.close();
        }
    }

    /** Hands the PDF at {@code path} to the system's print UI; an error message, or null. */
    static String printPdf(final String title, final String path) {
        final Activity activity = top;
        if (activity == null) {
            return "no activity is in front to print from";
        }
        RnBridge.main().post(new Runnable() {
            @Override
            public void run() {
                android.print.PrintManager manager = activity.getSystemService(android.print.PrintManager.class);
                if (manager != null) {
                    manager.print(title, new PdfAdapter(title, path), null);
                }
            }
        });
        return null;
    }

    /** Streams a finished PDF to the print framework. */
    static final class PdfAdapter extends android.print.PrintDocumentAdapter {
        private final String title;
        private final String path;

        PdfAdapter(String title, String path) {
            this.title = title;
            this.path = path;
        }

        @Override
        public void onLayout(android.print.PrintAttributes old, android.print.PrintAttributes attributes,
                             android.os.CancellationSignal cancel, LayoutResultCallback callback,
                             Bundle extras) {
            callback.onLayoutFinished(new android.print.PrintDocumentInfo.Builder(title)
                .setContentType(android.print.PrintDocumentInfo.CONTENT_TYPE_DOCUMENT).build(), true);
        }

        @Override
        public void onWrite(android.print.PageRange[] pages, android.os.ParcelFileDescriptor destination,
                            android.os.CancellationSignal cancel, WriteResultCallback callback) {
            try (InputStream in = new java.io.FileInputStream(path);
                 OutputStream out = new FileOutputStream(destination.getFileDescriptor())) {
                byte[] buffer = new byte[8192];
                for (int read; (read = in.read(buffer)) > 0; ) {
                    out.write(buffer, 0, read);
                }
                callback.onWriteFinished(new android.print.PageRange[] {android.print.PageRange.ALL_PAGES});
            } catch (Exception e) {
                callback.onWriteFailed(e.toString());
            }
        }
    }

    // ---- Push: Firebase Cloud Messaging, when the application ships it. ----

    /**
     * The registration token, subscribing to {@code topics}; throws with
     * the reason when FCM is absent or refuses. Called off the main thread
     * (it waits for Google Play services).
     */
    static String pushToken(String[] topics) throws Exception {
        Class<?> messaging;
        try {
            messaging = Class.forName("com.google.firebase.messaging.FirebaseMessaging");
        } catch (ClassNotFoundException e) {
            throw new UnsupportedOperationException("Firebase Cloud Messaging is not in this application: "
                + "add firebase-messaging and its google-services.json to push");
        }
        Object instance = messaging.getMethod("getInstance").invoke(null);
        Class<?> tasks = Class.forName("com.google.android.gms.tasks.Tasks");
        Class<?> task = Class.forName("com.google.android.gms.tasks.Task");
        java.lang.reflect.Method await = tasks.getMethod("await", task);
        for (String topic : topics) {
            await.invoke(null, messaging.getMethod("subscribeToTopic", String.class).invoke(instance, topic));
        }
        return (String) await.invoke(null, messaging.getMethod("getToken").invoke(instance));
    }

    /** Whether Play Billing's library is in this application. */
    static boolean billingPresent() {
        try {
            Class.forName("com.android.billingclient.api.BillingClient");
            return true;
        } catch (ClassNotFoundException e) {
            return false;
        }
    }

    private static byte[] drain(InputStream in) throws java.io.IOException {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        byte[] buffer = new byte[8192];
        for (int read; (read = in.read(buffer)) > 0; ) {
            out.write(buffer, 0, read);
        }
        return out.toByteArray();
    }
}
