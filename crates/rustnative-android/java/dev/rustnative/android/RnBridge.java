package dev.rustnative.android;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.MessageQueue;
import android.view.Choreographer;
import android.view.View;

/**
 * The one door between Java and Rust.
 *
 * <p>Every {@code native} method here is registered by the Rust library's
 * {@code JNI_OnLoad} ({@code rustnative_android::export_main!}); every call
 * into Rust happens on the main thread, except {@link #nativeRunTests},
 * which the instrumentation thread makes. Rust calls the static helpers
 * below — and those of {@link RnViews}, {@link RnStyle}, {@link RnMeasure},
 * and {@link RnServices} — through JNI.
 */
public final class RnBridge {
    private RnBridge() {}

    /** The manifest meta-data naming the library to load. */
    static final String LIBRARY_META = "dev.rustnative.library";

    /**
     * While true, view listeners do not report: Rust sets it around every
     * change it makes itself, so a framework-made change never echoes back
     * as an event.
     */
    static boolean muted;

    private static boolean loaded;
    private static Handler main;

    /** Loads the application's Rust library, once per process. */
    public static synchronized void load(Context context) {
        if (loaded) {
            return;
        }
        String library = "main";
        try {
            ApplicationInfo info = context.getPackageManager()
                .getApplicationInfo(context.getPackageName(), PackageManager.GET_META_DATA);
            if (info.metaData != null && info.metaData.getString(LIBRARY_META) != null) {
                library = info.metaData.getString(LIBRARY_META);
            }
        } catch (PackageManager.NameNotFoundException ignored) {
            // Our own package is always found.
        }
        System.loadLibrary(library);
        main = new Handler(Looper.getMainLooper());
        loaded = true;
        RnServices.init(context);
        nativeInit(context.getApplicationContext());
    }

    static Handler main() {
        if (main == null) {
            main = new Handler(Looper.getMainLooper());
        }
        return main;
    }

    // ---- Helpers Rust calls. ----

    /** Sets {@link #muted}. */
    static void setMuted(boolean value) {
        muted = value;
    }

    /** Runs {@link #nativeIdle} once the main thread has nothing else to do. */
    static void scheduleIdle() {
        Looper.myQueue().addIdleHandler(new MessageQueue.IdleHandler() {
            @Override
            public boolean queueIdle() {
                nativeIdle();
                return false;
            }
        });
    }

    /** Runs {@link #nativeFrame} at the next display frame. */
    static void requestFrame(final long window) {
        Choreographer.getInstance().postFrameCallback(new Choreographer.FrameCallback() {
            @Override
            public void doFrame(long frameTimeNanos) {
                nativeFrame(window, frameTimeNanos);
            }
        });
    }

    /** Runs {@link #nativeTimer} after {@code delayMillis}. */
    static void schedule(final long token, long delayMillis) {
        main().postDelayed(new Runnable() {
            @Override
            public void run() {
                nativeTimer(token);
            }
        }, delayMillis);
    }

    /** The display density: device pixels per dp. */
    static float density(Context context) {
        return context.getResources().getDisplayMetrics().density;
    }

    /** Milliseconds of uptime: the clock `MotionEvent.getEventTime` uses. */
    static long uptimeMillis() {
        return android.os.SystemClock.uptimeMillis();
    }

    /** Whether {@code activity} is the device suite's activity. */
    static boolean isTestActivity(Activity activity) {
        return activity instanceof RnTestActivity;
    }

    /** The API level the device runs. */
    static int apiLevel() {
        return android.os.Build.VERSION.SDK_INT;
    }

    /** Logs through the platform log, under the given tag. */
    static void log(int priority, String tag, String message) {
        android.util.Log.println(priority, tag, message);
    }

    /** Starts window {@code window} as a new activity, next to {@code from}. */
    static void openWindow(Activity from, long window, String title) {
        Intent intent = new Intent(from, RnWindowActivity.class);
        intent.putExtra(RnActivity.EXTRA_WINDOW, window);
        intent.putExtra(RnActivity.EXTRA_TITLE, title);
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_DOCUMENT | Intent.FLAG_ACTIVITY_MULTIPLE_TASK);
        if (from.isInMultiWindowMode()) {
            intent.addFlags(Intent.FLAG_ACTIVITY_LAUNCH_ADJACENT);
        }
        from.startActivity(intent);
    }

    // ---- Natives (registered by Rust). ----

    /** The process loaded the library. */
    static native void nativeInit(Context application);

    /**
     * An activity was created for window {@code window} (0 for the launcher
     * activity), with {@code root} its content view; {@code restored} when
     * the system recreated it from saved state.
     */
    static native void nativeCreate(Activity activity, long window, RnLayout root, boolean restored,
        Intent intent);

    /** A lifecycle step ({@code Rn.LC_*}), with its argument. */
    static native void nativeLifecycle(long window, int what, int argument);

    /** The window's content area is now {@code width} × {@code height} pixels. */
    static native void nativeResized(long window, int width, int height);

    /**
     * The window's insets changed: system bars, display cutout, IME, and
     * system gesture insets, each as left, top, right, bottom, in pixels.
     */
    static native void nativeInsets(long window, int[] insets);

    /** A view event ({@code Rn.EV_*}) on the view tagged {@code tag}. */
    static native void nativeViewEvent(long window, int tag, int event, long a, long b, String text);

    /** A key event; returns whether the framework took it. */
    static native boolean nativeKey(long window, int action, int keyCode, int meta, int unicode,
        int repeat, int source);

    /**
     * A pointer event on the window's root, before any view sees it: the
     * action, the index of the pointer it concerns, then per pointer its
     * id, tool type, and x, y, pressure, tilt, orientation (pixels and
     * radians), with the buttons and meta state. Returns whether the
     * framework claims the event.
     */
    static native boolean nativePointer(long window, int action, int actionIndex, int[] ids,
        int[] tools, float[] values, int buttons, int meta, long eventTime);

    /**
     * A game controller's button ({@code axes} null) or axes; returns
     * whether a node took it.
     */
    static native boolean nativeGamepad(long window, int device, int keyCode, int action,
        float[] axes);

    /** A custom text target's input method step: 1 compose, 2 commit, 3 cancel. */
    static native void nativeText(long window, int tag, int step, String text, int cursor);

    /** A back gesture's phase ({@code Rn.BACK_*}). */
    static native void nativeBack(long window, int phase, float progress, int edge);

    /** An intent reached a running activity (a deep link, a share). */
    static native void nativeIntent(long window, Intent intent);

    /** An activity result arrived. */
    static native void nativeActivityResult(int request, int result, Intent data);

    /** A permission request was answered. */
    static native void nativePermissions(int request, String[] permissions, int[] results);

    /** The main thread is idle and idle work was asked for. */
    static native void nativeIdle();

    /** A display frame for window {@code window}. */
    static native void nativeFrame(long window, long frameTimeNanos);

    /** A timer Rust scheduled fired. */
    static native void nativeTimer(long token);

    /** An answer to work Rust ran off the main thread (a service). */
    static native void nativeServiceReply(long token, int status, String text, byte[] bytes);

    /** Runs background job {@code name}; true asks for it to run again later. */
    static native boolean nativeRunJob(String name);

    /** Runs the device suite (instrumentation thread). */
    static native void nativeRunTests(android.app.Instrumentation instrumentation, String filter);
}
