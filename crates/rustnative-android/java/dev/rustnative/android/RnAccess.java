package dev.rustnative.android;

import android.graphics.Rect;
import android.os.Build;
import android.os.Bundle;
import android.view.MotionEvent;
import android.view.View;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityManager;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeProvider;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.List;

/**
 * The portable accessibility model on {@link AccessibilityNodeInfo}
 * ({@code accessibility.rs}): one {@link Delegate} per view carries what
 * the model says about it — role (as the class name services read, or a
 * role description), name, description, value, states, position in a set,
 * relations, automation id — and turns the actions a screen reader performs
 * into portable actions. A view whose node declares virtual elements (a
 * canvas's drawn regions) gets a {@link Provider} exposing them as virtual
 * views.
 */
final class RnAccess {
    private RnAccess() {}

    // Flags (`accessibility.rs`'s `flags`).
    static final int CHECKABLE = 1;
    static final int CHECKED = 2;
    static final int MIXED = 4;
    static final int EXPANDABLE = 8;
    static final int EXPANDED = 16;
    static final int SELECTED = 32;
    static final int BUSY = 64;
    static final int READ_ONLY = 128;
    static final int REQUIRED = 256;
    static final int HEADING = 512;
    static final int INVOKE = 1024;
    static final int RANGE = 2048;
    static final int SELECTABLE = 4096;
    static final int HIDDEN = 8192;
    static final int FOCUSABLE = 16384;
    static final int TEXT_VALUE = 32768;

    // Portable actions, as reported (`EV_ACCESSIBILITY`'s `a`).
    static final int ACT_INVOKE = 1;
    static final int ACT_INCREMENT = 2;
    static final int ACT_DECREMENT = 3;
    static final int ACT_EXPAND = 4;
    static final int ACT_COLLAPSE = 5;
    static final int ACT_TOGGLE = 6;
    static final int ACT_SELECT = 7;
    static final int ACT_SET_VALUE = 8;
    static final int ACT_SET_RANGE = 9;
    static final int ACT_SCROLL_INTO_VIEW = 10;
    static final int ACT_FOCUS = 11;

    /** The extra TalkBack reads a role description from. */
    static final String ROLE_DESCRIPTION = "AccessibilityNodeInfo.roleDescription";

    /** What the model says about one view (or one virtual element). */
    static final class Spec {
        String className;
        String role;
        String name;
        String description;
        String state;
        String id;
        int flags;
        int index = -1;
        int size;
        float min;
        float max;
        float current;
        View labeledBy;
        Rect bounds = new Rect();

        void apply(View host, AccessibilityNodeInfo info) {
            if (className != null) {
                info.setClassName(className);
            }
            if (role != null) {
                info.getExtras().putCharSequence(ROLE_DESCRIPTION, role);
            }
            if (name != null) {
                boolean shown = host instanceof TextView
                    && name.equals(String.valueOf(((TextView) host).getText()));
                if (!shown) {
                    info.setContentDescription(name);
                }
            }
            if (description != null) {
                if (Build.VERSION.SDK_INT >= 26) {
                    info.setHintText(description);
                } else {
                    info.setTooltipText(description);
                }
            }
            String stateText = state;
            if ((flags & REQUIRED) != 0) {
                stateText = stateText == null ? "Required" : stateText + ", required";
            }
            if ((flags & BUSY) != 0) {
                stateText = stateText == null ? "Busy" : stateText + ", busy";
            }
            if (stateText != null && Build.VERSION.SDK_INT >= 30) {
                info.setStateDescription(stateText);
            }
            if ((flags & CHECKABLE) != 0) {
                info.setCheckable(true);
                info.setChecked((flags & CHECKED) != 0);
            }
            if ((flags & SELECTABLE) != 0) {
                info.setSelected((flags & SELECTED) != 0);
            }
            if ((flags & HEADING) != 0 && Build.VERSION.SDK_INT >= 28) {
                info.setHeading(true);
            }
            if ((flags & READ_ONLY) != 0) {
                info.setEditable(false);
            }
            if ((flags & RANGE) != 0) {
                info.setRangeInfo(AccessibilityNodeInfo.RangeInfo.obtain(
                    AccessibilityNodeInfo.RangeInfo.RANGE_TYPE_FLOAT, min, max, current));
                info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_PROGRESS);
                info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_FORWARD);
                info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_BACKWARD);
            }
            if ((flags & EXPANDABLE) != 0) {
                info.addAction((flags & EXPANDED) != 0
                    ? AccessibilityNodeInfo.AccessibilityAction.ACTION_COLLAPSE
                    : AccessibilityNodeInfo.AccessibilityAction.ACTION_EXPAND);
            }
            if ((flags & INVOKE) != 0) {
                info.setClickable(true);
                info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_CLICK);
            }
            if ((flags & FOCUSABLE) != 0) {
                info.setFocusable(true);
            }
            if ((flags & TEXT_VALUE) != 0 && (flags & READ_ONLY) == 0) {
                info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_TEXT);
            }
            if (index >= 0) {
                info.setCollectionItemInfo(AccessibilityNodeInfo.CollectionItemInfo.obtain(
                    index, 1, 0, 1, (flags & HEADING) != 0, (flags & SELECTED) != 0));
            }
            if (labeledBy != null) {
                info.setLabeledBy(labeledBy);
            }
            if (id != null) {
                info.setViewIdResourceName(id);
            }
        }

        /** The portable action an Android action stands for, or 0. */
        int action(int code, Bundle arguments) {
            if (code == AccessibilityNodeInfo.ACTION_CLICK && (flags & INVOKE) != 0) {
                return (flags & CHECKABLE) != 0 ? ACT_TOGGLE : ACT_INVOKE;
            }
            if (code == AccessibilityNodeInfo.ACTION_EXPAND && (flags & EXPANDABLE) != 0) {
                return ACT_EXPAND;
            }
            if (code == AccessibilityNodeInfo.ACTION_COLLAPSE && (flags & EXPANDABLE) != 0) {
                return ACT_COLLAPSE;
            }
            if ((flags & RANGE) != 0) {
                if (code == AccessibilityNodeInfo.ACTION_SCROLL_FORWARD) {
                    return ACT_INCREMENT;
                }
                if (code == AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD) {
                    return ACT_DECREMENT;
                }
                if (code == android.R.id.accessibilityActionSetProgress) {
                    return ACT_SET_RANGE;
                }
            }
            if (code == AccessibilityNodeInfo.ACTION_SET_TEXT && (flags & TEXT_VALUE) != 0) {
                return ACT_SET_VALUE;
            }
            if (code == AccessibilityNodeInfo.ACTION_SELECT && (flags & SELECTABLE) != 0) {
                return ACT_SELECT;
            }
            return 0;
        }
    }

    /** Carries one view's spec, and its virtual elements' provider. */
    static final class Delegate extends View.AccessibilityDelegate {
        final long window;
        final int tag;
        final Spec spec = new Spec();
        Provider provider;

        Delegate(long window, int tag) {
            this.window = window;
            this.tag = tag;
        }

        @Override
        public void onInitializeAccessibilityNodeInfo(View host, AccessibilityNodeInfo info) {
            super.onInitializeAccessibilityNodeInfo(host, info);
            spec.apply(host, info);
        }

        @Override
        public boolean performAccessibilityAction(View host, int action, Bundle arguments) {
            // A platform widget's own action is its own: the widget acts,
            // and its listener reports what changed, as for a tap or drag.
            // The framework's containers and canvases have no actions of
            // their own; theirs are the portable ones.
            boolean framework = host instanceof RnLayout || host instanceof RnCanvasView;
            if (!framework && super.performAccessibilityAction(host, action, arguments)) {
                return true;
            }
            int portable = spec.action(action, arguments);
            if (portable != 0) {
                report(window, tag, portable, -1, value(action, arguments));
                return true;
            }
            return framework && super.performAccessibilityAction(host, action, arguments);
        }

        @Override
        public AccessibilityNodeProvider getAccessibilityNodeProvider(View host) {
            return provider;
        }
    }

    static String value(int action, Bundle arguments) {
        if (arguments == null) {
            return null;
        }
        if (action == android.R.id.accessibilityActionSetProgress) {
            return Float.toString(arguments.getFloat(
                AccessibilityNodeInfo.ACTION_ARGUMENT_PROGRESS_VALUE));
        }
        if (action == AccessibilityNodeInfo.ACTION_SET_TEXT) {
            CharSequence text = arguments.getCharSequence(
                AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE);
            return text == null ? "" : text.toString();
        }
        return null;
    }

    static void report(long window, int tag, int action, int element, String value) {
        RnBridge.nativeViewEvent(window, tag, Rn.EV_ACCESSIBILITY, action, element, value);
    }

    /** Empty strings stand for "not said" across the arrays Rust sends. */
    static String said(String text) {
        return text == null || text.isEmpty() ? null : text;
    }

    static Delegate delegate(View view, long window, int tag) {
        View.AccessibilityDelegate current = view.getAccessibilityDelegate();
        if (current instanceof Delegate) {
            return (Delegate) current;
        }
        Delegate delegate = new Delegate(window, tag);
        view.setAccessibilityDelegate(delegate);
        return delegate;
    }

    /**
     * Applies a node's model to its view. {@code strings}: class name, role
     * description, name, description, state, automation id, pane title.
     * {@code ints}: flags, live region (0 off, 1 polite, 2 assertive), index
     * in set, set size. {@code floats}: min, max, current.
     */
    static void apply(View view, long window, int tag, String[] strings, int[] ints,
        float[] floats) {
        Delegate delegate = delegate(view, window, tag);
        Spec spec = delegate.spec;
        spec.className = said(strings[0]);
        spec.role = said(strings[1]);
        spec.name = said(strings[2]);
        spec.description = said(strings[3]);
        spec.state = said(strings[4]);
        spec.id = said(strings[5]);
        spec.flags = ints[0];
        spec.index = ints[2];
        spec.size = ints[3];
        spec.min = floats[0];
        spec.max = floats[1];
        spec.current = floats[2];
        int live = ints[1] == 2 ? View.ACCESSIBILITY_LIVE_REGION_ASSERTIVE
            : ints[1] == 1 ? View.ACCESSIBILITY_LIVE_REGION_POLITE
            : View.ACCESSIBILITY_LIVE_REGION_NONE;
        if (view.getAccessibilityLiveRegion() != live) {
            view.setAccessibilityLiveRegion(live);
        }
        int importance = (spec.flags & HIDDEN) != 0
            ? View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
            : View.IMPORTANT_FOR_ACCESSIBILITY_AUTO;
        if ((spec.flags & HIDDEN) == 0 && (spec.className != null || spec.name != null
            || (spec.flags & (INVOKE | RANGE | CHECKABLE | HEADING)) != 0)) {
            importance = View.IMPORTANT_FOR_ACCESSIBILITY_YES;
        }
        if (view.getImportantForAccessibility() != importance) {
            view.setImportantForAccessibility(importance);
        }
        if (Build.VERSION.SDK_INT >= 28) {
            String pane = said(strings[6]);
            CharSequence current = view.getAccessibilityPaneTitle();
            if (pane == null ? current != null : !pane.contentEquals(current == null ? "" : current)) {
                view.setAccessibilityPaneTitle(pane);
            }
        }
        if (view.getParent() != null) {
            view.getParent().notifySubtreeAccessibilityStateChanged(view, view,
                AccessibilityEvent.CONTENT_CHANGE_TYPE_UNDEFINED);
        }
    }

    /** Sets which view labels {@code view} (null: none). */
    static void setLabeledBy(View view, long window, int tag, View label) {
        delegate(view, window, tag).spec.labeledBy = label;
    }

    /** Announces {@code text} now (an assertive live region's change). */
    @SuppressWarnings("deprecation")
    static void announce(View view, String text) {
        view.announceForAccessibility(text);
    }

    /**
     * Sets the virtual elements of {@code view}: per element, its strings
     * (class name, role, name, description, state, id), ints (flags, index,
     * size), bounds (left, top, right, bottom in the view's pixels), and
     * floats (min, max, current).
     */
    static void setElements(View view, long window, int tag, String[] strings, int[] ints,
        int[] bounds, float[] floats) {
        Delegate delegate = delegate(view, window, tag);
        int count = ints.length / 3;
        if (count == 0) {
            delegate.provider = null;
            return;
        }
        if (delegate.provider == null) {
            delegate.provider = new Provider(view, delegate);
        }
        List<Spec> elements = new ArrayList<>();
        for (int i = 0; i < count; i++) {
            Spec spec = new Spec();
            spec.className = said(strings[i * 6]);
            spec.role = said(strings[i * 6 + 1]);
            spec.name = said(strings[i * 6 + 2]);
            spec.description = said(strings[i * 6 + 3]);
            spec.state = said(strings[i * 6 + 4]);
            spec.id = said(strings[i * 6 + 5]);
            spec.flags = ints[i * 3];
            spec.index = ints[i * 3 + 1];
            spec.size = ints[i * 3 + 2];
            spec.bounds = new Rect(bounds[i * 4], bounds[i * 4 + 1], bounds[i * 4 + 2],
                bounds[i * 4 + 3]);
            spec.min = floats[i * 3];
            spec.max = floats[i * 3 + 1];
            spec.current = floats[i * 3 + 2];
            elements.add(spec);
        }
        delegate.provider.elements = elements;
        view.sendAccessibilityEvent(AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED);
    }

    /** Exposes a view's virtual elements as virtual views. */
    static final class Provider extends AccessibilityNodeProvider {
        final View host;
        final Delegate delegate;
        List<Spec> elements = new ArrayList<>();
        int focused = Integer.MIN_VALUE;
        int hovered = Integer.MIN_VALUE;

        Provider(View host, Delegate delegate) {
            this.host = host;
            this.delegate = delegate;
        }

        @Override
        public AccessibilityNodeInfo createAccessibilityNodeInfo(int virtualId) {
            if (virtualId == View.NO_ID) {
                AccessibilityNodeInfo info = AccessibilityNodeInfo.obtain(host);
                host.onInitializeAccessibilityNodeInfo(info);
                for (int i = 0; i < elements.size(); i++) {
                    info.addChild(host, i);
                }
                return info;
            }
            if (virtualId < 0 || virtualId >= elements.size()) {
                return null;
            }
            Spec spec = elements.get(virtualId);
            AccessibilityNodeInfo info = AccessibilityNodeInfo.obtain(host, virtualId);
            info.setParent(host);
            info.setPackageName(host.getContext().getPackageName());
            info.setClassName(spec.className != null ? spec.className : "android.view.View");
            info.setBoundsInParent(spec.bounds);
            int[] location = new int[2];
            host.getLocationOnScreen(location);
            Rect screen = new Rect(spec.bounds);
            screen.offset(location[0], location[1]);
            info.setBoundsInScreen(screen);
            info.setVisibleToUser(host.isShown());
            info.setEnabled(host.isEnabled());
            info.setFocusable(true);
            info.setAccessibilityFocused(focused == virtualId);
            info.addAction(focused == virtualId
                ? AccessibilityNodeInfo.AccessibilityAction.ACTION_CLEAR_ACCESSIBILITY_FOCUS
                : AccessibilityNodeInfo.AccessibilityAction.ACTION_ACCESSIBILITY_FOCUS);
            spec.apply(host, info);
            if (spec.name != null) {
                info.setContentDescription(spec.name);
            }
            return info;
        }

        @Override
        public boolean performAction(int virtualId, int action, Bundle arguments) {
            if (virtualId == View.NO_ID) {
                return host.performAccessibilityAction(action, arguments);
            }
            if (virtualId < 0 || virtualId >= elements.size()) {
                return false;
            }
            if (action == AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS) {
                if (focused != virtualId) {
                    int previous = focused;
                    focused = virtualId;
                    send(previous, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED);
                    send(virtualId, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUSED);
                }
                return true;
            }
            if (action == AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS) {
                if (focused == virtualId) {
                    focused = Integer.MIN_VALUE;
                    send(virtualId, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED);
                }
                return true;
            }
            int portable = elements.get(virtualId).action(action, arguments);
            if (portable == 0 && action == AccessibilityNodeInfo.ACTION_CLICK) {
                portable = ACT_INVOKE;
            }
            if (portable != 0) {
                report(delegate.window, delegate.tag, portable, virtualId, value(action, arguments));
                return true;
            }
            return false;
        }

        void send(int virtualId, int type) {
            if (virtualId < 0 || host.getParent() == null) {
                return;
            }
            AccessibilityEvent event = AccessibilityEvent.obtain(type);
            event.setPackageName(host.getContext().getPackageName());
            event.setSource(host, virtualId);
            host.getParent().requestSendAccessibilityEvent(host, event);
        }

        /** Explore-by-touch: the element under a hovering finger. */
        boolean hover(MotionEvent event) {
            AccessibilityManager manager = (AccessibilityManager)
                host.getContext().getSystemService(android.content.Context.ACCESSIBILITY_SERVICE);
            if (manager == null || !manager.isTouchExplorationEnabled()) {
                return false;
            }
            int found = Integer.MIN_VALUE;
            for (int i = elements.size() - 1; i >= 0; i--) {
                if (elements.get(i).bounds.contains((int) event.getX(), (int) event.getY())) {
                    found = i;
                    break;
                }
            }
            if (event.getAction() == MotionEvent.ACTION_HOVER_EXIT) {
                found = Integer.MIN_VALUE;
            }
            if (found != hovered) {
                send(found, AccessibilityEvent.TYPE_VIEW_HOVER_ENTER);
                send(hovered, AccessibilityEvent.TYPE_VIEW_HOVER_EXIT);
                hovered = found;
            }
            return found != Integer.MIN_VALUE;
        }
    }

    /** Routes a hover to the view's virtual elements; whether one took it. */
    static boolean dispatchHover(View view, MotionEvent event) {
        View.AccessibilityDelegate delegate = view.getAccessibilityDelegate();
        if (delegate instanceof Delegate && ((Delegate) delegate).provider != null) {
            return ((Delegate) delegate).provider.hover(event);
        }
        return false;
    }
}
