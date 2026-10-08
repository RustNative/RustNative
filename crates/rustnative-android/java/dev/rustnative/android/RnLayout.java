package dev.rustnative.android;

import android.content.Context;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;

/**
 * A container whose children are placed exactly where the portable layout
 * engine put them. Rust computes every rectangle; this view only measures
 * each child at its rectangle's size and lays it out at its position,
 * clipping what overflows.
 *
 * <p>A window's root is an {@code RnLayout} too: it reports its size and
 * insets, and offers every pointer event to the framework first.
 */
public class RnLayout extends ViewGroup {
    /** A child's rectangle, in this container's pixels. */
    static final class Params extends ViewGroup.LayoutParams {
        int x;
        int y;

        Params() {
            super(0, 0);
        }
    }

    long window;
    boolean root;
    /** The content size, when this lays out a scroll container's content. */
    int contentWidth = -1;
    int contentHeight = -1;

    public RnLayout(Context context) {
        super(context);
        setClipChildren(true);
        setClipToPadding(true);
    }

    /** Makes this the root of window {@code window}. */
    void becomeRoot(long window) {
        this.window = window;
        this.root = true;
        setOnApplyWindowInsetsListener(new OnApplyWindowInsetsListener() {
            @Override
            public WindowInsets onApplyWindowInsets(View view, WindowInsets insets) {
                RnBridge.nativeInsets(RnLayout.this.window, RnInsets.read(insets));
                return insets;
            }
        });
    }

    /** Places {@code child} at {@code (x, y)} with size {@code width} × {@code height}. */
    static void place(View child, int x, int y, int width, int height) {
        ViewGroup.LayoutParams current = child.getLayoutParams();
        Params params = current instanceof Params ? (Params) current : new Params();
        if (params != current || params.x != x || params.y != y || params.width != width
            || params.height != height) {
            params.x = x;
            params.y = y;
            params.width = width;
            params.height = height;
            child.setLayoutParams(params);
        }
    }

    /** Sets the content size a scroll container scrolls over. */
    static void setContentSize(RnLayout layout, int width, int height) {
        if (layout.contentWidth != width || layout.contentHeight != height) {
            layout.contentWidth = width;
            layout.contentHeight = height;
            layout.requestLayout();
        }
    }

    /** Inserts {@code child} at {@code index} (or at the end). */
    static void insert(ViewGroup parent, View child, int index) {
        ViewGroup old = (ViewGroup) child.getParent();
        if (old == parent) {
            int current = parent.indexOfChild(child);
            if (current == index || (index < 0 && current == parent.getChildCount() - 1)) {
                return;
            }
            parent.removeViewInLayout(child);
        } else if (old != null) {
            old.removeView(child);
        }
        if (child.getLayoutParams() == null || !(child.getLayoutParams() instanceof Params)) {
            child.setLayoutParams(new Params());
        }
        if (index < 0 || index > parent.getChildCount()) {
            parent.addView(child);
        } else {
            parent.addView(child, index);
        }
    }

    /** Removes {@code child} from whatever holds it. */
    static void detach(View child) {
        ViewGroup parent = (ViewGroup) child.getParent();
        if (parent != null) {
            parent.removeView(child);
        }
    }

    @Override
    protected void onMeasure(int widthSpec, int heightSpec) {
        int width = contentWidth >= 0 ? contentWidth : sized(widthSpec);
        int height = contentHeight >= 0 ? contentHeight : sized(heightSpec);
        if (contentWidth >= 0 && MeasureSpec.getMode(widthSpec) == MeasureSpec.EXACTLY) {
            width = Math.max(width, MeasureSpec.getSize(widthSpec));
        }
        if (contentHeight >= 0 && MeasureSpec.getMode(heightSpec) == MeasureSpec.EXACTLY) {
            height = Math.max(height, MeasureSpec.getSize(heightSpec));
        }
        for (int i = 0; i < getChildCount(); i++) {
            View child = getChildAt(i);
            ViewGroup.LayoutParams params = child.getLayoutParams();
            int childWidth = Math.max(0, params.width);
            int childHeight = Math.max(0, params.height);
            child.measure(MeasureSpec.makeMeasureSpec(childWidth, MeasureSpec.EXACTLY),
                MeasureSpec.makeMeasureSpec(childHeight, MeasureSpec.EXACTLY));
        }
        setMeasuredDimension(width, height);
    }

    private static int sized(int spec) {
        return MeasureSpec.getMode(spec) == MeasureSpec.UNSPECIFIED ? 0 : MeasureSpec.getSize(spec);
    }

    @Override
    protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
        for (int i = 0; i < getChildCount(); i++) {
            View child = getChildAt(i);
            ViewGroup.LayoutParams raw = child.getLayoutParams();
            if (!(raw instanceof Params)) {
                continue;
            }
            Params params = (Params) raw;
            child.layout(params.x, params.y, params.x + Math.max(0, params.width),
                params.y + Math.max(0, params.height));
        }
    }

    @Override
    protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        if (root) {
            RnBridge.nativeResized(window, width, height);
        }
    }

    @Override
    public boolean dispatchTouchEvent(MotionEvent event) {
        if (root && RnPointer.offer(window, event)) {
            return true;
        }
        return super.dispatchTouchEvent(event);
    }

    @Override
    public boolean dispatchGenericMotionEvent(MotionEvent event) {
        if (root && RnPointer.offer(window, event)) {
            return true;
        }
        return super.dispatchGenericMotionEvent(event);
    }

    @Override
    protected ViewGroup.LayoutParams generateDefaultLayoutParams() {
        return new Params();
    }

    @Override
    protected boolean checkLayoutParams(ViewGroup.LayoutParams params) {
        return params instanceof Params;
    }

    @Override
    public boolean shouldDelayChildPressedState() {
        return false;
    }

    @Override
    public boolean dispatchHoverEvent(android.view.MotionEvent event) {
        // A screen reader exploring by touch reaches virtual elements here.
        return RnAccess.dispatchHover(this, event) || super.dispatchHoverEvent(event);
    }

    @Override
    public boolean onCheckIsTextEditor() {
        // A focused framework view is a custom text target (`RnInput`).
        return isFocused() && RnViews.tagOf(this) != null;
    }

    @Override
    public android.view.inputmethod.InputConnection onCreateInputConnection(
        android.view.inputmethod.EditorInfo info) {
        return RnInput.connect(this, info);
    }
}
