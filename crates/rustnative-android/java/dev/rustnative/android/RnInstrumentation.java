package dev.rustnative.android;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Context;
import android.content.Intent;
import android.os.Bundle;

/**
 * Runs the device suite (`rustnative_android`'s `device_tests`) inside a
 * real activity: `am instrument -r -w <package>/dev.rustnative.android.RnInstrumentation`.
 * Each test's start and outcome is reported with {@link #sendStatus} in the
 * shape `am instrument -r` prints, which `tools/android-device-test.sh`
 * reads. A `filter` argument (`-e filter <text>`) runs only the tests whose
 * names contain it.
 */
public class RnInstrumentation extends Instrumentation {
    private Bundle arguments;
    private int passed;
    private int failed;

    @Override
    public void onCreate(Bundle arguments) {
        super.onCreate(arguments);
        this.arguments = arguments;
        start();
    }

    @Override
    public void onStart() {
        super.onStart();
        Context context = getTargetContext();
        Intent intent = new Intent(context, RnTestActivity.class);
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TASK);
        Activity activity = startActivitySync(intent);
        waitForIdleSync();
        String filter = arguments != null ? arguments.getString("filter") : null;
        RnBridge.nativeRunTests(this, filter == null ? "" : filter);
        activity.finish();
        Bundle results = new Bundle();
        results.putString(Instrumentation.REPORT_KEY_STREAMRESULT,
            "\nRust Native device suite: " + passed + " passed, " + failed + " failed\n");
        results.putInt("passed", passed);
        results.putInt("failed", failed);
        finish(failed == 0 ? Activity.RESULT_OK : Activity.RESULT_CANCELED, results);
    }

    /**
     * The UI automation, connected without suppressing accessibility
     * services, so a running TalkBack stays running while the suite reads
     * the tree.
     */
    android.app.UiAutomation automation() {
        android.app.UiAutomation automation =
            getUiAutomation(android.app.UiAutomation.FLAG_DONT_SUPPRESS_ACCESSIBILITY_SERVICES);
        android.accessibilityservice.AccessibilityServiceInfo info = automation.getServiceInfo();
        int wanted = android.accessibilityservice.AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS;
        if (info != null && (info.flags & wanted) == 0) {
            info.flags |= wanted;
            automation.setServiceInfo(info);
        }
        return automation;
    }

    /** Presses and releases a game controller's button (instrumentation thread). */
    void gamepadButton(int code) {
        long now = android.os.SystemClock.uptimeMillis();
        for (int action : new int[] {android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.ACTION_UP}) {
            sendKeySync(new android.view.KeyEvent(now, now, action, code, 0, 0, -1, 0, 0,
                android.view.InputDevice.SOURCE_GAMEPAD));
        }
    }

    /**
     * Composes {@code composing}, then commits {@code committed}, through
     * the connection a focused custom text target offers an input method
     * (main thread).
     */
    static boolean ime(android.view.View view, String composing, String committed) {
        android.view.inputmethod.InputConnection connection =
            RnInput.connect(view, new android.view.inputmethod.EditorInfo());
        if (connection == null) {
            return false;
        }
        connection.setComposingText(composing, 1);
        connection.commitText(committed, 1);
        return true;
    }

    /**
     * The root of this application's window: the active window may be
     * another one (a screen reader's own tutorial over it, say).
     */
    android.view.accessibility.AccessibilityNodeInfo ourRoot() {
        android.app.UiAutomation automation = automation();
        String ours = getTargetContext().getPackageName();
        for (android.view.accessibility.AccessibilityWindowInfo window : automation.getWindows()) {
            android.view.accessibility.AccessibilityNodeInfo root = window.getRoot();
            if (root != null && ours.contentEquals(root.getPackageName())) {
                return root;
            }
        }
        return automation.getRootInActiveWindow();
    }

    /**
     * The active window's accessibility tree, one node per line:
     * depth, class, text, content description, view id, flags (c checkable,
     * C checked, s selected, h heading, k clickable, f focusable, a
     * accessibility-focused, e enabled), range (min:max:current), state
     * description, the text of the node labelling it, role description,
     * bounds on screen — separated by tabs.
     */
    String dumpAccessibility() {
        android.view.accessibility.AccessibilityNodeInfo root = null;
        for (int attempt = 0; attempt < 50 && root == null; attempt++) {
            root = ourRoot();
            if (root == null) {
                android.os.SystemClock.sleep(100);
            }
        }
        StringBuilder out = new StringBuilder();
        if (root != null) {
            dump(root, 0, out);
        }
        return out.toString();
    }

    private static void dump(android.view.accessibility.AccessibilityNodeInfo node, int depth,
        StringBuilder out) {
        StringBuilder flags = new StringBuilder();
        if (node.isCheckable()) flags.append('c');
        if (node.isChecked()) flags.append('C');
        if (node.isSelected()) flags.append('s');
        if (android.os.Build.VERSION.SDK_INT >= 28 && node.isHeading()) flags.append('h');
        if (node.isClickable()) flags.append('k');
        if (node.isFocusable()) flags.append('f');
        if (node.isAccessibilityFocused()) flags.append('a');
        if (node.isEnabled()) flags.append('e');
        android.view.accessibility.AccessibilityNodeInfo.RangeInfo range = node.getRangeInfo();
        android.view.accessibility.AccessibilityNodeInfo labeledBy = node.getLabeledBy();
        android.graphics.Rect bounds = new android.graphics.Rect();
        node.getBoundsInScreen(bounds);
        CharSequence role = node.getExtras().getCharSequence(RnAccess.ROLE_DESCRIPTION);
        CharSequence state = android.os.Build.VERSION.SDK_INT >= 30 ? node.getStateDescription() : null;
        out.append(depth).append('\t')
            .append(node.getClassName()).append('\t')
            .append(clean(node.getText())).append('\t')
            .append(clean(node.getContentDescription())).append('\t')
            .append(clean(node.getViewIdResourceName())).append('\t')
            .append(flags).append('\t')
            .append(range == null ? "" : range.getMin() + ":" + range.getMax() + ":" + range.getCurrent())
            .append('\t').append(clean(state)).append('\t')
            .append(labeledBy == null ? "" : clean(labeledBy.getText())).append('\t')
            .append(clean(role)).append('\t')
            .append(bounds.flattenToString()).append('\n');
        for (int i = 0; i < node.getChildCount(); i++) {
            android.view.accessibility.AccessibilityNodeInfo child = node.getChild(i);
            if (child != null) {
                dump(child, depth + 1, out);
            }
        }
    }

    private static String clean(CharSequence text) {
        return text == null ? "" : text.toString().replace('\t', ' ').replace('\n', ' ');
    }

    /**
     * Performs accessibility {@code action} (with a progress {@code value}
     * for {@code ACTION_SET_PROGRESS}) on the first node whose view id, or
     * else content description, is {@code key}, the way a screen reader
     * does. Returns whether a node took it.
     */
    boolean performAccessibilityAction(String key, int action, float value) {
        android.view.accessibility.AccessibilityNodeInfo root = ourRoot();
        android.view.accessibility.AccessibilityNodeInfo node = root == null ? null : find(root, key);
        if (node == null) {
            return false;
        }
        android.os.Bundle arguments = new android.os.Bundle();
        arguments.putFloat(
            android.view.accessibility.AccessibilityNodeInfo.ACTION_ARGUMENT_PROGRESS_VALUE, value);
        return node.performAction(action, arguments);
    }

    private static android.view.accessibility.AccessibilityNodeInfo find(
        android.view.accessibility.AccessibilityNodeInfo node, String key) {
        if (key.equals(node.getViewIdResourceName())
            || (node.getContentDescription() != null
                && key.contentEquals(node.getContentDescription()))) {
            return node;
        }
        for (int i = 0; i < node.getChildCount(); i++) {
            android.view.accessibility.AccessibilityNodeInfo child = node.getChild(i);
            if (child != null) {
                android.view.accessibility.AccessibilityNodeInfo found = find(child, key);
                if (found != null) {
                    return found;
                }
            }
        }
        return null;
    }

    /** Brings the test activity back in front of whatever covers it. */
    void bringToFront() {
        Intent intent = new Intent(getTargetContext(), RnTestActivity.class);
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_REORDER_TO_FRONT);
        getTargetContext().startActivity(intent);
        waitForIdleSync();
    }

    /** Whether a screen reader is exploring by touch now. */
    boolean touchExploration() {
        android.view.accessibility.AccessibilityManager manager =
            (android.view.accessibility.AccessibilityManager) getTargetContext()
                .getSystemService(android.content.Context.ACCESSIBILITY_SERVICE);
        return manager != null && manager.isTouchExplorationEnabled();
    }

    /** Runs {@code command} as the shell user and returns its output (Rust calls it). */
    String shell(String command) {
        android.os.ParcelFileDescriptor output = automation().executeShellCommand(command);
        StringBuilder text = new StringBuilder();
        try (java.io.InputStream stream =
                 new android.os.ParcelFileDescriptor.AutoCloseInputStream(output)) {
            byte[] buffer = new byte[4096];
            int read;
            while ((read = stream.read(buffer)) > 0) {
                text.append(new String(buffer, 0, read, java.nio.charset.StandardCharsets.UTF_8));
            }
        } catch (java.io.IOException failed) {
            return "";
        }
        return text.toString();
    }

    /** Reports one test: status 1 started, 0 passed, -2 failed (Rust calls it). */
    void report(String name, int status, String message) {
        Bundle bundle = new Bundle();
        bundle.putString("class", "rustnative");
        bundle.putString("test", name);
        if (status == 0) {
            passed++;
        } else if (status == -2) {
            failed++;
            bundle.putString("stack", message);
        }
        if (message != null) {
            bundle.putString(Instrumentation.REPORT_KEY_STREAMRESULT,
                (status == -2 ? "FAILED " : "") + name + ": " + message + "\n");
        }
        sendStatus(status, bundle);
    }
}
