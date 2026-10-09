package dev.rustnative.android;

import android.content.Context;

/**
 * Library-only mode (Milestone 40): an existing Android application links a
 * Rust Native library for its application model and services — the data
 * layer, sync, secure storage — and shows no framework UI. {@link #start}
 * loads the library (`export_main!()` with no `main`) and connects the host
 * library, so the services work from the application's own JNI calls.
 */
public final class RnLibrary {
    private RnLibrary() {}

    /** Loads the Rust library once per process (call it from {@code Application.onCreate}). */
    public static void start(Context context) {
        RnBridge.load(context);
    }
}
