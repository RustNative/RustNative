package dev.rustnative.android;

import android.graphics.Insets;
import android.os.Build;
import android.view.DisplayCutout;
import android.view.WindowInsets;

/** Reads a window's insets into the flat form Rust takes. */
final class RnInsets {
    private RnInsets() {}

    /**
     * Sixteen pixels: system bars, display cutout, IME, and system gestures,
     * each as left, top, right, bottom.
     */
    static int[] read(WindowInsets insets) {
        int[] out = new int[16];
        if (Build.VERSION.SDK_INT >= 30) {
            put(out, 0, insets.getInsets(WindowInsets.Type.systemBars()));
            put(out, 4, insets.getInsets(WindowInsets.Type.displayCutout()));
            put(out, 8, insets.getInsets(WindowInsets.Type.ime()));
            put(out, 12, insets.getInsets(WindowInsets.Type.systemGestures()));
            return out;
        }
        out[0] = insets.getSystemWindowInsetLeft();
        out[1] = insets.getSystemWindowInsetTop();
        out[2] = insets.getSystemWindowInsetRight();
        out[3] = insets.getSystemWindowInsetBottom();
        if (Build.VERSION.SDK_INT >= 28) {
            DisplayCutout cutout = insets.getDisplayCutout();
            if (cutout != null) {
                out[4] = cutout.getSafeInsetLeft();
                out[5] = cutout.getSafeInsetTop();
                out[6] = cutout.getSafeInsetRight();
                out[7] = cutout.getSafeInsetBottom();
            }
        }
        if (Build.VERSION.SDK_INT >= 29) {
            Insets gestures = insets.getSystemGestureInsets();
            out[12] = gestures.left;
            out[13] = gestures.top;
            out[14] = gestures.right;
            out[15] = gestures.bottom;
        }
        return out;
    }

    private static void put(int[] out, int at, Insets insets) {
        out[at] = insets.left;
        out[at + 1] = insets.top;
        out[at + 2] = insets.right;
        out[at + 3] = insets.bottom;
    }
}
