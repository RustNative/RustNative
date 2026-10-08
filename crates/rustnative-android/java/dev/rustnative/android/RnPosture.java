package dev.rustnative.android;

import android.app.Activity;
import android.graphics.Rect;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.List;
import java.util.WeakHashMap;

/**
 * A foldable's hinge, read from the window extensions the device ships in
 * its system image ({@code androidx.window.extensions}, declared optional in
 * the manifest) — by reflection, so the application carries no AndroidX
 * library. Devices without a hinge, and devices whose image has no
 * extensions, answer "no folding features": a flat posture.
 *
 * <p>Each folding feature is reported as type (1 fold, 2 hinge), state
 * (1 flat, 2 half-opened), and its bounds in the window, in pixels.
 */
final class RnPosture {
    private RnPosture() {}

    private static final WeakHashMap<Activity, int[]> LATEST = new WeakHashMap<>();
    private static Object component;
    private static boolean probed;

    /** The window layout component, or null where the device has none. */
    private static synchronized Object component() {
        if (probed) {
            return component;
        }
        probed = true;
        try {
            Class<?> provider = Class.forName("androidx.window.extensions.WindowExtensionsProvider");
            Object extensions = provider.getMethod("getWindowExtensions").invoke(null);
            component = extensions.getClass().getMethod("getWindowLayoutComponent").invoke(extensions);
        } catch (Throwable absent) {
            component = null;
        }
        return component;
    }

    /** Whether the device reports folding features at all. */
    static boolean available() {
        return component() != null;
    }

    /**
     * Starts following {@code activity}'s folding features; each change is
     * reported with {@link RnBridge#nativeLifecycle} ({@code Rn.LC_POSTURE}).
     */
    static void watch(final Activity activity, final long window) {
        Object layout = component();
        if (layout == null) {
            return;
        }
        try {
            Method add = null;
            for (Method method : layout.getClass().getMethods()) {
                if (method.getName().equals("addWindowLayoutInfoListener")
                    && method.getParameterTypes().length == 2
                    && method.getParameterTypes()[0] == Activity.class) {
                    add = method;
                    break;
                }
            }
            if (add == null) {
                return;
            }
            Class<?> consumerType = add.getParameterTypes()[1];
            Object consumer = Proxy.newProxyInstance(consumerType.getClassLoader(),
                new Class<?>[] {consumerType}, new InvocationHandler() {
                    @Override
                    public Object invoke(Object proxy, Method method, Object[] args) {
                        if (method.getName().equals("accept") && args != null && args.length == 1) {
                            synchronized (LATEST) {
                                LATEST.put(activity, describe(args[0]));
                            }
                            RnBridge.main().post(new Runnable() {
                                @Override
                                public void run() {
                                    RnBridge.nativeLifecycle(window, Rn.LC_POSTURE, 0);
                                }
                            });
                        } else if (method.getName().equals("hashCode")) {
                            return System.identityHashCode(proxy);
                        } else if (method.getName().equals("equals")) {
                            return proxy == args[0];
                        }
                        return null;
                    }
                });
            add.invoke(layout, activity, consumer);
        } catch (Throwable unsupported) {
            // An extensions version this reflection does not know: flat.
        }
    }

    /** The folding features last reported for {@code activity}: 6 ints each. */
    static int[] features(Activity activity) {
        synchronized (LATEST) {
            int[] latest = LATEST.get(activity);
            return latest != null ? latest : new int[0];
        }
    }

    private static int[] describe(Object info) {
        try {
            List<?> features = (List<?>) info.getClass().getMethod("getDisplayFeatures").invoke(info);
            int count = 0;
            int[] out = new int[features.size() * 6];
            for (Object feature : features) {
                Class<?> type = feature.getClass();
                Rect bounds = (Rect) type.getMethod("getBounds").invoke(feature);
                int kind = 0;
                int state = 0;
                try {
                    kind = (Integer) type.getMethod("getType").invoke(feature);
                    state = (Integer) type.getMethod("getState").invoke(feature);
                } catch (NoSuchMethodException notFolding) {
                    continue;
                }
                out[count * 6] = kind;
                out[count * 6 + 1] = state;
                out[count * 6 + 2] = bounds.left;
                out[count * 6 + 3] = bounds.top;
                out[count * 6 + 4] = bounds.right;
                out[count * 6 + 5] = bounds.bottom;
                count++;
            }
            int[] trimmed = new int[count * 6];
            System.arraycopy(out, 0, trimmed, 0, trimmed.length);
            return trimmed;
        } catch (Throwable unreadable) {
            return new int[0];
        }
    }

    /** The display's size in pixels (the window's share of it is split-screen). */
    static int[] display(Activity activity) {
        android.util.DisplayMetrics metrics = new android.util.DisplayMetrics();
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            Rect bounds = activity.getWindowManager().getMaximumWindowMetrics().getBounds();
            return new int[] {bounds.width(), bounds.height()};
        }
        activity.getWindowManager().getDefaultDisplay().getRealMetrics(metrics);
        return new int[] {metrics.widthPixels, metrics.heightPixels};
    }
}
