package dev.rustnative.android;

import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.graphics.drawable.StateListDrawable;
import android.os.Build;
import android.util.TypedValue;
import android.view.View;
import android.view.ViewOutlineProvider;
import android.widget.TextView;
import java.util.WeakHashMap;

/**
 * Realizes a node's resolved style on its real widget, the way Android
 * applications style their own views: a {@link GradientDrawable} per state
 * (fill, stroke, corner radius) in a {@link StateListDrawable}, wrapped in
 * the host's ripple for pressable controls; a {@link ColorStateList} for
 * the text; the typeface and size; elevation for a shadow. Only what Rust
 * says differs from the host's look is set; everything else is restored to
 * what the widget had when it was created, so an unstyled control keeps the
 * device theme's own look.
 */
final class RnStyle {
    private RnStyle() {}

    /** What a widget looked like before the framework styled it. */
    private static final class Defaults {
        Drawable background;
        ColorStateList textColors;
        Typeface typeface;
        float textSize;
        int[] padding;
        float elevation;
        ViewOutlineProvider outline;
        boolean clipToOutline;
    }

    private static final WeakHashMap<View, Defaults> DEFAULTS = new WeakHashMap<>();

    private static Defaults defaults(View view) {
        Defaults defaults = DEFAULTS.get(view);
        if (defaults == null) {
            defaults = new Defaults();
            defaults.background = view.getBackground();
            if (view instanceof TextView) {
                TextView text = (TextView) view;
                defaults.textColors = text.getTextColors();
                defaults.typeface = text.getTypeface();
                defaults.textSize = text.getTextSize();
            }
            defaults.padding = new int[] {view.getPaddingLeft(), view.getPaddingTop(),
                view.getPaddingRight(), view.getPaddingBottom()};
            defaults.elevation = view.getElevation();
            defaults.outline = view.getOutlineProvider();
            defaults.clipToOutline = view.getClipToOutline();
            DEFAULTS.put(view, defaults);
        }
        return defaults;
    }

    /**
     * Applies a style. {@code ints} holds {@link Rn#STATES} entries of
     * {@link Rn#STATE_INTS} (flags, background, foreground, border);
     * {@code floats} holds {@link Rn#STATES} entries of
     * {@link Rn#STATE_FLOATS} (corner radius and elevation, in pixels).
     * {@code padding} is start, top, end, bottom pixels added to the
     * widget's own, or null for the widget's own alone. Start and end
     * follow the view's layout direction.
     */
    static void apply(View view, int kind, int[] ints, float[] floats, float borderWidth,
        int[] padding, float fontSize, int weight, String family) {
        Defaults defaults = defaults(view);
        int any = 0;
        for (int state = 0; state < Rn.STATES; state++) {
            any |= ints[state * Rn.STATE_INTS];
        }
        // The box: fill, stroke, radius.
        if ((any & (Rn.ST_BACKGROUND | Rn.ST_BORDER | Rn.ST_RADIUS)) == 0) {
            if (view.getBackground() != defaults.background) {
                view.setBackground(defaults.background);
            }
        } else {
            view.setBackground(background(view, kind, ints, floats, borderWidth, defaults));
        }
        // Elevation (a shadow), over an outline that follows the radius.
        int normal = ints[0];
        if ((normal & Rn.ST_ELEVATION) != 0) {
            view.setElevation(floats[1]);
            view.setOutlineProvider(ViewOutlineProvider.BACKGROUND);
        } else {
            view.setElevation(defaults.elevation);
            view.setOutlineProvider(defaults.outline);
        }
        // A container clips its children to its rounded corners.
        boolean clip = (normal & Rn.ST_RADIUS) != 0 && floats[0] > 0
            && (kind == Rn.CONTAINER || kind == Rn.SCROLL || kind == Rn.IMAGE);
        if (clip) {
            view.setOutlineProvider(ViewOutlineProvider.BACKGROUND);
        }
        view.setClipToOutline(clip || defaults.clipToOutline);
        // Text.
        if (view instanceof TextView) {
            TextView text = (TextView) view;
            if ((any & Rn.ST_FOREGROUND) != 0) {
                text.setTextColor(textColors(ints, defaults.textColors));
            } else if (text.getTextColors() != defaults.textColors) {
                text.setTextColor(defaults.textColors);
            }
            applyFont(text, fontSize, weight, family);
        }
        int[] own = defaults.padding;
        if (padding == null) {
            if (view.getPaddingLeft() != own[0] || view.getPaddingTop() != own[1]
                || view.getPaddingRight() != own[2] || view.getPaddingBottom() != own[3]) {
                view.setPadding(own[0], own[1], own[2], own[3]);
            }
        } else {
            // A widget's own horizontal padding is symmetric, so its left
            // stands for its start in either direction.
            int start = own[0] + padding[0];
            int top = own[1] + padding[1];
            int end = own[2] + padding[2];
            int bottom = own[3] + padding[3];
            if (view.getPaddingStart() != start || view.getPaddingTop() != top
                || view.getPaddingEnd() != end || view.getPaddingBottom() != bottom) {
                view.setPaddingRelative(start, top, end, bottom);
            }
        }
    }

    private static Drawable background(View view, int kind, int[] ints, float[] floats,
        float borderWidth, Defaults defaults) {
        StateListDrawable states = new StateListDrawable();
        // Most specific first: a state list takes the first match.
        int[][] specs = {
            {-android.R.attr.state_enabled},
            {android.R.attr.state_pressed},
            {android.R.attr.state_focused},
            {android.R.attr.state_hovered},
            {},
        };
        int[] order = {Rn.STATE_DISABLED, Rn.STATE_PRESSED, Rn.STATE_FOCUSED, Rn.STATE_HOVERED,
            Rn.STATE_NORMAL};
        boolean pressedStyled = false;
        for (int i = 0; i < order.length; i++) {
            int state = order[i];
            int flags = ints[state * Rn.STATE_INTS];
            if (state != Rn.STATE_NORMAL && (flags & (Rn.ST_BACKGROUND | Rn.ST_BORDER)) == 0) {
                continue;
            }
            if (state == Rn.STATE_PRESSED) {
                pressedStyled = true;
            }
            states.addState(specs[i], box(ints, floats, state, borderWidth));
        }
        boolean pressable = kind == Rn.BUTTON || kind == Rn.LINK || kind == Rn.DATE
            || kind == Rn.CHECKBOX || kind == Rn.RADIO || kind == Rn.TOGGLE;
        if (pressable && !pressedStyled) {
            TypedValue highlight = new TypedValue();
            int color = 0x33000000;
            if (view.getContext().getTheme().resolveAttribute(android.R.attr.colorControlHighlight,
                highlight, true)) {
                color = highlight.data;
            }
            GradientDrawable mask = box(ints, floats, Rn.STATE_NORMAL, 0);
            mask.setColor(Color.WHITE);
            return new RippleDrawable(ColorStateList.valueOf(color), states, mask);
        }
        return states;
    }

    /** One state's box, falling back to the normal state for what it does not set. */
    private static GradientDrawable box(int[] ints, float[] floats, int state, float borderWidth) {
        int at = state * Rn.STATE_INTS;
        int flags = ints[at];
        int normal = ints[0];
        GradientDrawable box = new GradientDrawable();
        box.setShape(GradientDrawable.RECTANGLE);
        if ((flags & Rn.ST_BACKGROUND) != 0) {
            box.setColor(ints[at + 1]);
        } else if ((normal & Rn.ST_BACKGROUND) != 0) {
            box.setColor(ints[1]);
        } else {
            box.setColor(Color.TRANSPARENT);
        }
        if ((flags & Rn.ST_BORDER) != 0) {
            box.setStroke(Math.max(1, Math.round(borderWidth)), ints[at + 3]);
        } else if ((normal & Rn.ST_BORDER) != 0) {
            box.setStroke(Math.max(1, Math.round(borderWidth)), ints[3]);
        }
        float radius = (flags & Rn.ST_RADIUS) != 0 ? floats[state * Rn.STATE_FLOATS]
            : (normal & Rn.ST_RADIUS) != 0 ? floats[0] : 0;
        box.setCornerRadius(radius);
        return box;
    }

    private static ColorStateList textColors(int[] ints, ColorStateList defaults) {
        int fallback = defaults != null ? defaults.getDefaultColor() : Color.BLACK;
        int normal = (ints[0] & Rn.ST_FOREGROUND) != 0 ? ints[2] : fallback;
        int[][] specs = {
            {-android.R.attr.state_enabled},
            {android.R.attr.state_pressed},
            {android.R.attr.state_focused},
            {android.R.attr.state_hovered},
            {},
        };
        int[] order = {Rn.STATE_DISABLED, Rn.STATE_PRESSED, Rn.STATE_FOCUSED, Rn.STATE_HOVERED,
            Rn.STATE_NORMAL};
        int[] colors = new int[order.length];
        for (int i = 0; i < order.length; i++) {
            int at = order[i] * Rn.STATE_INTS;
            if ((ints[at] & Rn.ST_FOREGROUND) != 0) {
                colors[i] = ints[at + 2];
            } else if (order[i] == Rn.STATE_DISABLED && defaults != null) {
                colors[i] = defaults.getColorForState(specs[0], normal);
            } else {
                colors[i] = normal;
            }
        }
        return new ColorStateList(specs, colors);
    }

    /**
     * Sets a text view's font: {@code size} pixels (0 keeps the theme's),
     * {@code weight} 100–900 (0 keeps it), {@code family} (null keeps it).
     */
    static void applyFont(TextView view, float size, int weight, String family) {
        Defaults defaults = defaults(view);
        float pixels = size > 0 ? size : defaults.textSize;
        if (view.getTextSize() != pixels) {
            view.setTextSize(TypedValue.COMPLEX_UNIT_PX, pixels);
        }
        Typeface face = family == null && weight == 0 ? defaults.typeface
            : typeface(family, weight, defaults.typeface);
        if (view.getTypeface() != face) {
            view.setTypeface(face);
        }
    }

    /** The typeface for {@code family} at {@code weight}, from {@code base}. */
    static Typeface typeface(String family, int weight, Typeface base) {
        Typeface face = family != null ? Typeface.create(family, Typeface.NORMAL)
            : base != null ? base : Typeface.DEFAULT;
        if (weight <= 0) {
            return face;
        }
        if (Build.VERSION.SDK_INT >= 28) {
            return Typeface.create(face, Math.max(1, Math.min(1000, weight)), false);
        }
        return Typeface.create(face, weight >= 600 ? Typeface.BOLD : Typeface.NORMAL);
    }
}
