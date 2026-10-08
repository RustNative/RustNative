package dev.rustnative.android;

import android.app.Activity;
import android.text.Layout;
import android.text.StaticLayout;
import android.text.TextPaint;
import android.util.SparseArray;
import android.view.View;
import android.widget.TextView;

/**
 * Measures nodes the way they will be drawn: each kind on a prototype of
 * the very widget it is realized as, so the device theme's own padding,
 * minimum sizes, and fonts produce the number; and bare text through
 * {@link StaticLayout}, the host's own line breaker and shaper.
 */
final class RnMeasure {
    private RnMeasure() {}

    private static final SparseArray<View> PROTOTYPES = new SparseArray<>();
    private static Activity owner;

    /** Forgets the prototypes (the theme or the font scale changed). */
    static void forget() {
        PROTOTYPES.clear();
        owner = null;
    }

    private static View prototype(Activity activity, int kind) {
        if (owner != activity) {
            PROTOTYPES.clear();
            owner = activity;
        }
        View view = PROTOTYPES.get(kind);
        if (view == null) {
            view = RnViews.create(activity, kind, 0, -1, 2);
            view.setTag(null);
            // A text view relayouts through its layout parameters when its
            // text changes; a prototype has no parent to give it any.
            view.setLayoutParams(new android.view.ViewGroup.LayoutParams(
                android.view.ViewGroup.LayoutParams.WRAP_CONTENT,
                android.view.ViewGroup.LayoutParams.WRAP_CONTENT));
            PROTOTYPES.put(kind, view);
        }
        return view;
    }

    /**
     * Measures a node of {@code kind} showing {@code text} (and
     * {@code items}, for a choice), at most {@code maxWidth} pixels wide
     * ({@code -1} for unbounded), in the given font ({@code fontSize} 0
     * keeps the theme's). Returns width and height, in pixels.
     */
    static int[] measure(Activity activity, int kind, String text, String[] items, int maxWidth,
        float fontSize, int weight, String family) {
        View view = prototype(activity, kind);
        RnBridge.muted = true;
        try {
            if (view instanceof TextView && text != null) {
                ((TextView) view).setText(text);
            }
            if (items != null) {
                RnViews.setOptions(view, items, items.length > 0 ? 0 : -1);
            }
            if (view instanceof TextView) {
                RnStyle.applyFont((TextView) view, fontSize, weight, family);
            }
        } finally {
            RnBridge.muted = false;
        }
        int widthSpec = maxWidth < 0
            ? View.MeasureSpec.makeMeasureSpec(0, View.MeasureSpec.UNSPECIFIED)
            : View.MeasureSpec.makeMeasureSpec(maxWidth, View.MeasureSpec.AT_MOST);
        view.measure(widthSpec, View.MeasureSpec.makeMeasureSpec(0, View.MeasureSpec.UNSPECIFIED));
        return new int[] {view.getMeasuredWidth(), view.getMeasuredHeight()};
    }

    /**
     * Measures bare text through {@link StaticLayout}: width and height in
     * pixels, and the line count.
     */
    static int[] text(Activity activity, String text, int maxWidth, float fontSize, int weight,
        String family) {
        TextView reference = (TextView) prototype(activity, Rn.LABEL);
        TextPaint paint = new TextPaint(reference.getPaint());
        if (fontSize > 0) {
            paint.setTextSize(fontSize);
        }
        paint.setTypeface(RnStyle.typeface(family, weight, paint.getTypeface()));
        int width = maxWidth < 0
            ? (int) Math.ceil(Layout.getDesiredWidth(text, paint))
            : maxWidth;
        StaticLayout layout = StaticLayout.Builder.obtain(text, 0, text.length(), paint,
            Math.max(1, width)).setIncludePad(true).build();
        float widest = 0;
        for (int line = 0; line < layout.getLineCount(); line++) {
            widest = Math.max(widest, layout.getLineWidth(line));
        }
        return new int[] {(int) Math.ceil(widest), layout.getHeight(), layout.getLineCount()};
    }
}
