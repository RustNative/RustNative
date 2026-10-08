package dev.rustnative.android;

import android.animation.ValueAnimator;
import android.app.Activity;
import android.app.UiModeManager;
import android.content.Context;
import android.content.pm.PackageManager;
import android.content.res.Configuration;
import android.content.res.TypedArray;
import android.os.Build;
import android.provider.Settings;
import android.util.DisplayMetrics;
import android.view.InputDevice;

/**
 * What the device and the person's settings say: the host traits Rust
 * feeds into the environment ({@code host_traits.rs}), and the device facts
 * capabilities are answered from.
 */
final class RnPlatform {
    private RnPlatform() {}

    /**
     * The host traits: density, font scale, night mode (1/0), right to left
     * (1/0), animations enabled (1/0), contrast (-1 when the host does not
     * say), high-text-contrast (1/0), the window's width and height in
     * pixels, and multi-window (1/0).
     */
    static float[] traits(Activity activity) {
        Configuration configuration = activity.getResources().getConfiguration();
        DisplayMetrics metrics = activity.getResources().getDisplayMetrics();
        float night = (configuration.uiMode & Configuration.UI_MODE_NIGHT_MASK)
            == Configuration.UI_MODE_NIGHT_YES ? 1 : 0;
        float rtl = configuration.getLayoutDirection() == android.view.View.LAYOUT_DIRECTION_RTL
            ? 1 : 0;
        float animations = ValueAnimator.areAnimatorsEnabled() ? 1 : 0;
        float contrast = -1;
        if (Build.VERSION.SDK_INT >= 34) {
            UiModeManager modes = (UiModeManager) activity.getSystemService(Context.UI_MODE_SERVICE);
            if (modes != null) {
                contrast = modes.getContrast();
            }
        }
        float highText = 0;
        try {
            highText = Settings.Secure.getInt(activity.getContentResolver(),
                "high_text_contrast_enabled", 0);
        } catch (SecurityException ignored) {
            // Not readable here: unknown is "off".
        }
        return new float[] {
            metrics.density,
            configuration.fontScale,
            night,
            rtl,
            animations,
            contrast,
            highText,
            activity.getWindow().getDecorView().getWidth(),
            activity.getWindow().getDecorView().getHeight(),
            activity.isInMultiWindowMode() ? 1 : 0,
        };
    }

    /** The current locale, as a BCP 47 tag. */
    static String locale(Activity activity) {
        return activity.getResources().getConfiguration().getLocales().get(0).toLanguageTag();
    }

    /**
     * The theme's colours, as ARGB: accent, on accent, surface, on surface,
     * highlight, border, muted text.
     */
    static int[] colors(Activity activity) {
        int[] attributes = {
            android.R.attr.colorAccent,
            android.R.attr.textColorPrimaryInverse,
            android.R.attr.colorBackground,
            android.R.attr.textColorPrimary,
            android.R.attr.colorControlHighlight,
            android.R.attr.colorControlNormal,
            android.R.attr.textColorSecondary,
        };
        int[] out = new int[attributes.length];
        for (int i = 0; i < attributes.length; i++) {
            TypedArray values = activity.obtainStyledAttributes(new int[] {attributes[i]});
            try {
                out[i] = values.getColor(0, 0);
            } finally {
                values.recycle();
            }
        }
        if (Build.VERSION.SDK_INT >= 31) {
            // The person's wallpaper-derived accent (dynamic colour).
            out[0] = activity.getColor(android.R.color.system_accent1_600);
        }
        return out;
    }

    /**
     * Device facts: API level, a hardware keyboard (1/0), a stylus (1/0),
     * a mouse (1/0), a gamepad (1/0), touch (1/0), a camera (1/0),
     * Play services (1/0), a WebView (1/0), printing (1/0), the camera usable now
     * (present and permitted, 1/0).
     */
    static int[] device(Context activity) {
        boolean keyboard = activity.getResources().getConfiguration().keyboard
            == Configuration.KEYBOARD_QWERTY;
        boolean stylus = false;
        boolean mouse = false;
        boolean gamepad = false;
        for (int id : InputDevice.getDeviceIds()) {
            InputDevice device = InputDevice.getDevice(id);
            if (device == null) {
                continue;
            }
            int sources = device.getSources();
            stylus |= (sources & InputDevice.SOURCE_STYLUS) == InputDevice.SOURCE_STYLUS;
            mouse |= (sources & InputDevice.SOURCE_MOUSE) == InputDevice.SOURCE_MOUSE
                && !device.isVirtual();
            gamepad |= (sources & InputDevice.SOURCE_GAMEPAD) == InputDevice.SOURCE_GAMEPAD;
        }
        PackageManager packages = activity.getPackageManager();
        boolean play = false;
        try {
            packages.getPackageInfo("com.google.android.gms", 0);
            play = true;
        } catch (PackageManager.NameNotFoundException ignored) {
            // No Play services.
        }
        boolean web = Build.VERSION.SDK_INT >= 26
            && android.webkit.WebView.getCurrentWebViewPackage() != null;
        return new int[] {
            Build.VERSION.SDK_INT,
            keyboard ? 1 : 0,
            stylus ? 1 : 0,
            mouse ? 1 : 0,
            gamepad ? 1 : 0,
            packages.hasSystemFeature(PackageManager.FEATURE_TOUCHSCREEN) ? 1 : 0,
            packages.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY) ? 1 : 0,
            play ? 1 : 0,
            web ? 1 : 0,
            packages.hasSystemFeature(PackageManager.FEATURE_PRINTING) ? 1 : 0,
            RnHost.cameraAllowed(activity) ? 1 : 0,
        };
    }
}
