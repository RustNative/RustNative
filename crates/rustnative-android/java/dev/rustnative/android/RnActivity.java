package dev.rustnative.android;

import android.app.Activity;
import android.content.Intent;
import android.content.res.Configuration;
import android.os.Build;
import android.os.Bundle;
import android.view.KeyEvent;
import android.view.Menu;
import android.view.MenuItem;
import android.window.BackEvent;
import android.window.OnBackAnimationCallback;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;

/**
 * One window of a Rust Native application.
 *
 * <p>The launcher activity is window 0 until Rust names it (the primary
 * window); another window opens as another instance of this class, carrying
 * its window id in {@link #EXTRA_WINDOW}. The activity owns nothing of the
 * application: it forwards its lifecycle, input, and results to Rust, and
 * hosts the {@link RnLayout} Rust realizes the window's tree into. The
 * application outlives it — a configuration change is handled in place, and
 * an activity the system recreates reattaches to the same application.
 */
public class RnActivity extends Activity {
    /** The window an activity shows (absent for the launcher activity). */
    public static final String EXTRA_WINDOW = "dev.rustnative.window";
    /** The title of a window opened by the application. */
    public static final String EXTRA_TITLE = "dev.rustnative.title";

    long window;
    RnLayout root;
    private boolean backHandled;
    private Object backCallback;
    /** The options menu Rust describes, as parallel arrays (`RnMenus`). */
    RnMenus.Model menu;

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        RnBridge.load(this);
        window = getIntent().getLongExtra(EXTRA_WINDOW, 0);
        String title = getIntent().getStringExtra(EXTRA_TITLE);
        if (title != null) {
            setTitle(title);
        }
        // Edge to edge: the framework places content clear of the system
        // bars and cutouts itself, from the insets (`SAFE_AREA`).
        if (Build.VERSION.SDK_INT >= 30) {
            getWindow().setDecorFitsSystemWindows(false);
        } else {
            getWindow().getDecorView().setSystemUiVisibility(
                android.view.View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | android.view.View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    | android.view.View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
        }
        if (Build.VERSION.SDK_INT >= 28) {
            getWindow().getAttributes().layoutInDisplayCutoutMode =
                android.view.WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
        }
        root = new RnLayout(this);
        setContentView(root);
        RnBridge.nativeCreate(this, window, root, saved != null, getIntent());
    }

    /** Rust names the window this activity shows once it is attached. */
    void attach(long window) {
        this.window = window;
        root.becomeRoot(window);
        root.requestApplyInsets();
    }

    @Override
    protected void onStart() {
        super.onStart();
        RnBridge.nativeLifecycle(window, Rn.LC_START, 0);
    }

    @Override
    protected void onResume() {
        super.onResume();
        RnBridge.nativeLifecycle(window, Rn.LC_RESUME, 0);
    }

    @Override
    protected void onPause() {
        RnBridge.nativeLifecycle(window, Rn.LC_PAUSE, 0);
        super.onPause();
    }

    @Override
    protected void onStop() {
        RnBridge.nativeLifecycle(window, Rn.LC_STOP, 0);
        super.onStop();
    }

    @Override
    protected void onDestroy() {
        RnBridge.nativeLifecycle(window,
            isFinishing() && !isChangingConfigurations() ? Rn.LC_DESTROY_FINISHING : Rn.LC_DESTROY,
            0);
        super.onDestroy();
    }

    @Override
    protected void onSaveInstanceState(Bundle out) {
        super.onSaveInstanceState(out);
        RnBridge.nativeLifecycle(window, Rn.LC_SAVE, 0);
        out.putBoolean("dev.rustnative.saved", true);
    }

    @Override
    public void onConfigurationChanged(Configuration configuration) {
        super.onConfigurationChanged(configuration);
        RnBridge.nativeLifecycle(window, Rn.LC_CONFIGURATION, 0);
    }

    @Override
    public void onMultiWindowModeChanged(boolean multiWindow, Configuration configuration) {
        super.onMultiWindowModeChanged(multiWindow, configuration);
        RnBridge.nativeLifecycle(window, Rn.LC_MULTI_WINDOW, multiWindow ? 1 : 0);
    }

    @Override
    public void onTrimMemory(int level) {
        super.onTrimMemory(level);
        RnBridge.nativeLifecycle(window, Rn.LC_TRIM, level);
    }

    @Override
    public void onLowMemory() {
        super.onLowMemory();
        RnBridge.nativeLifecycle(window, Rn.LC_LOW_MEMORY, 0);
    }

    @Override
    public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        RnBridge.nativeLifecycle(window, Rn.LC_FOCUS, focused ? 1 : 0);
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        RnBridge.nativeIntent(window, intent);
    }

    @Override
    public boolean dispatchGenericMotionEvent(android.view.MotionEvent event) {
        if (event.isFromSource(android.view.InputDevice.SOURCE_JOYSTICK)
            && event.getActionMasked() == android.view.MotionEvent.ACTION_MOVE) {
            float[] axes = {
                event.getAxisValue(android.view.MotionEvent.AXIS_X),
                event.getAxisValue(android.view.MotionEvent.AXIS_Y),
                event.getAxisValue(android.view.MotionEvent.AXIS_Z),
                event.getAxisValue(android.view.MotionEvent.AXIS_RZ),
                Math.max(event.getAxisValue(android.view.MotionEvent.AXIS_LTRIGGER),
                    event.getAxisValue(android.view.MotionEvent.AXIS_BRAKE)),
                Math.max(event.getAxisValue(android.view.MotionEvent.AXIS_RTRIGGER),
                    event.getAxisValue(android.view.MotionEvent.AXIS_GAS)),
                event.getAxisValue(android.view.MotionEvent.AXIS_HAT_X),
                event.getAxisValue(android.view.MotionEvent.AXIS_HAT_Y),
            };
            if (RnBridge.nativeGamepad(window, event.getDeviceId(), 0, 0, axes)) {
                return true;
            }
        }
        return super.dispatchGenericMotionEvent(event);
    }

    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        if ((event.getSource() & android.view.InputDevice.SOURCE_GAMEPAD)
                == android.view.InputDevice.SOURCE_GAMEPAD
            && KeyEvent.isGamepadButton(event.getKeyCode())
            && RnBridge.nativeGamepad(window, event.getDeviceId(), event.getKeyCode(),
                event.getAction(), null)) {
            return true;
        }
        // The back key is the system's back (below), not a key event.
        if (event.getKeyCode() != KeyEvent.KEYCODE_BACK
            && RnBridge.nativeKey(window, event.getAction(), event.getKeyCode(),
                event.getMetaState(), event.getUnicodeChar(), event.getRepeatCount(),
                event.getSource())) {
            return true;
        }
        return super.dispatchKeyEvent(event);
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        RnBridge.nativeActivityResult(request, result, data);
    }

    @Override
    public void onRequestPermissionsResult(int request, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(request, permissions, results);
        RnBridge.nativePermissions(request, permissions, results);
    }

    // ---- Back. ----

    /**
     * Whether the application handles back right now. While it does, the
     * system's back gesture is the application's (with its predictive
     * progress); while it does not, the system's own back happens.
     */
    void setBackHandled(boolean handled) {
        if (handled == backHandled) {
            return;
        }
        backHandled = handled;
        if (Build.VERSION.SDK_INT < 33) {
            return;
        }
        OnBackInvokedDispatcher dispatcher = getOnBackInvokedDispatcher();
        if (handled) {
            Object callback = Build.VERSION.SDK_INT >= 34 ? animated() : simple();
            dispatcher.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT,
                (OnBackInvokedCallback) callback);
            backCallback = callback;
        } else if (backCallback != null) {
            dispatcher.unregisterOnBackInvokedCallback((OnBackInvokedCallback) backCallback);
            backCallback = null;
        }
    }

    private Object simple() {
        return new OnBackInvokedCallback() {
            @Override
            public void onBackInvoked() {
                RnBridge.nativeBack(window, Rn.BACK_INVOKED, 1f, 0);
            }
        };
    }

    private Object animated() {
        return new OnBackAnimationCallback() {
            @Override
            public void onBackStarted(BackEvent event) {
                RnBridge.nativeBack(window, Rn.BACK_STARTED, event.getProgress(),
                    event.getSwipeEdge() == BackEvent.EDGE_RIGHT ? 2 : 1);
            }

            @Override
            public void onBackProgressed(BackEvent event) {
                RnBridge.nativeBack(window, Rn.BACK_PROGRESSED, event.getProgress(), 0);
            }

            @Override
            public void onBackCancelled() {
                RnBridge.nativeBack(window, Rn.BACK_CANCELLED, 0f, 0);
            }

            @Override
            public void onBackInvoked() {
                RnBridge.nativeBack(window, Rn.BACK_INVOKED, 1f, 0);
            }
        };
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        // Before API 33 there is no callback: the key arrives here.
        if (backHandled && Build.VERSION.SDK_INT < 33) {
            RnBridge.nativeBack(window, Rn.BACK_INVOKED, 1f, 0);
            return;
        }
        super.onBackPressed();
    }

    // ---- The options menu (a window's menu bar). ----

    @Override
    public boolean onCreateOptionsMenu(Menu menu) {
        return RnMenus.populate(this, menu);
    }

    @Override
    public boolean onPrepareOptionsMenu(Menu menu) {
        return RnMenus.populate(this, menu);
    }

    @Override
    public boolean onOptionsItemSelected(MenuItem item) {
        if (RnMenus.choose(this, item)) {
            return true;
        }
        return super.onOptionsItemSelected(item);
    }
}
