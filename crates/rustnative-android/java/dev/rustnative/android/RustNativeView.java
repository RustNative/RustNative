package dev.rustnative.android;

import android.app.Activity;
import android.app.Application;
import android.content.Context;
import android.content.ContextWrapper;
import android.os.Bundle;
import android.util.AttributeSet;

/**
 * A Rust Native application's primary window inside an existing Android
 * application (embedding inward, Milestone 40): put it in a layout, or
 * create it in code, in any activity. When it is attached the library is
 * loaded and its exported {@code main} runs, and {@code AndroidPlatform::run}
 * realizes the application into this view instead of an activity of its
 * own. The host activity's lifecycle drives the application's.
 *
 * <pre>
 * &lt;dev.rustnative.android.RustNativeView
 *     android:layout_width="match_parent"
 *     android:layout_height="match_parent" /&gt;
 * </pre>
 *
 * The host keeps its own back handling, menus, and title; the embedded
 * application ends when the host activity finishes.
 */
public class RustNativeView extends RnLayout {
    private boolean started;

    public RustNativeView(Context context) {
        super(context);
    }

    public RustNativeView(Context context, AttributeSet attributes) {
        super(context);
    }

    static Activity activityOf(Context context) {
        while (context instanceof ContextWrapper) {
            if (context instanceof Activity) {
                return (Activity) context;
            }
            context = ((ContextWrapper) context).getBaseContext();
        }
        return null;
    }

    @Override
    protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        if (started) {
            return;
        }
        final Activity activity = activityOf(getContext());
        if (activity == null) {
            throw new IllegalStateException("a RustNativeView needs an Activity's context");
        }
        started = true;
        RnBridge.load(activity);
        activity.getApplication().registerActivityLifecycleCallbacks(new Lifecycle(activity));
        RnBridge.nativeCreate(activity, 0, this, false, activity.getIntent());
    }

    /** Forwards the host activity's lifecycle to the embedded window. */
    private final class Lifecycle implements Application.ActivityLifecycleCallbacks {
        private final Activity host;

        Lifecycle(Activity host) {
            this.host = host;
        }

        private void step(Activity activity, int what) {
            if (activity == host) {
                RnBridge.nativeLifecycle(window, what, 0);
            }
        }

        @Override public void onActivityCreated(Activity activity, Bundle saved) {}
        @Override public void onActivityStarted(Activity activity) { step(activity, Rn.LC_START); }
        @Override public void onActivityResumed(Activity activity) { step(activity, Rn.LC_RESUME); }
        @Override public void onActivityPaused(Activity activity) { step(activity, Rn.LC_PAUSE); }
        @Override public void onActivityStopped(Activity activity) { step(activity, Rn.LC_STOP); }
        @Override public void onActivitySaveInstanceState(Activity activity, Bundle out) {
            step(activity, Rn.LC_SAVE);
        }
        @Override public void onActivityDestroyed(Activity activity) {
            if (activity == host) {
                RnBridge.nativeLifecycle(window, activity.isFinishing() && !activity.isChangingConfigurations()
                    ? Rn.LC_DESTROY_FINISHING : Rn.LC_DESTROY, 0);
                activity.getApplication().unregisterActivityLifecycleCallbacks(this);
            }
        }
    }
}
