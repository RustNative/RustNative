package dev.rustnative.android;

import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.graphics.drawable.StateListDrawable;
import android.os.Build;
import android.view.View;
import android.widget.TextView;

/**
 * Reads back what the backend applied to a view, for the device suite and
 * the inspector: the realized style, not the requested one.
 */
final class RnProbe {
    private RnProbe() {}

    /**
     * The view's style: background colour (ARGB, 0 when the background is
     * not the framework's), text colour, text size (px), font weight, corner
     * radius (px), elevation (px), alpha, layout direction (1 right to left),
     * visibility (1 visible), enabled (1), left padding (px), right padding
     * (px).
     */
    static float[] style(View view) {
        float background = 0;
        float radius = 0;
        GradientDrawable box = box(view.getBackground());
        if (box != null) {
            if (box.getColor() != null) {
                background = Float.intBitsToFloat(box.getColor().getDefaultColor());
            }
            radius = box.getCornerRadius();
        }
        float text = 0;
        float size = 0;
        float weight = 0;
        if (view instanceof TextView) {
            TextView textView = (TextView) view;
            text = Float.intBitsToFloat(textView.getCurrentTextColor());
            size = textView.getTextSize();
            if (Build.VERSION.SDK_INT >= 28 && textView.getTypeface() != null) {
                weight = textView.getTypeface().getWeight();
            }
        }
        return new float[] {
            background, text, size, weight, radius, view.getElevation(), view.getAlpha(),
            view.getLayoutDirection() == View.LAYOUT_DIRECTION_RTL ? 1 : 0,
            view.getVisibility() == View.VISIBLE ? 1 : 0,
            view.isEnabled() ? 1 : 0,
            view.getPaddingLeft(), view.getPaddingRight(),
        };
    }

    private static GradientDrawable box(Drawable drawable) {
        if (drawable instanceof RippleDrawable && ((RippleDrawable) drawable).getNumberOfLayers() > 0) {
            drawable = ((RippleDrawable) drawable).getDrawable(0);
        }
        if (drawable instanceof StateListDrawable) {
            drawable = drawable.getCurrent();
        }
        return drawable instanceof GradientDrawable ? (GradientDrawable) drawable : null;
    }

    /** The class of the view's background drawable (null for none). */
    static String backgroundClass(View view) {
        Drawable background = view.getBackground();
        return background == null ? null : background.getClass().getName();
    }

    /** The view's identity (to tell the same view from a replacement). */
    static int identity(View view) {
        return System.identityHashCode(view);
    }
}
