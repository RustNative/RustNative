package dev.rustnative.android;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Typeface;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * A tab bar: a row of tabs, one selected, each reported as a tab to
 * accessibility services. Content switching is the framework's (every tab's
 * content stays mounted and hidden unless selected), so this view only
 * shows and reports the selection.
 */
final class RnTabs extends LinearLayout {
    private final RnViews.Tag tag;
    private int selected = -1;

    RnTabs(Context context, RnViews.Tag tag) {
        super(context);
        this.tag = tag;
        setOrientation(HORIZONTAL);
    }

    void set(String[] labels, int selected) {
        boolean same = getChildCount() == labels.length;
        for (int i = 0; same && i < labels.length; i++) {
            same = labels[i].equals(((TextView) getChildAt(i)).getText().toString());
        }
        if (!same) {
            removeAllViews();
            for (int i = 0; i < labels.length; i++) {
                addView(tab(labels[i], i), new LayoutParams(0, LayoutParams.MATCH_PARENT, 1f));
            }
        }
        this.selected = selected;
        for (int i = 0; i < getChildCount(); i++) {
            TextView tab = (TextView) getChildAt(i);
            boolean on = i == selected;
            tab.setSelected(on);
            tab.setTypeface(null, on ? Typeface.BOLD : Typeface.NORMAL);
        }
    }

    private TextView tab(String label, final int index) {
        TextView tab = new TextView(getContext()) {
            @Override
            public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
                super.onInitializeAccessibilityNodeInfo(info);
                info.setClassName("android.widget.TabWidget$Tab");
                info.setSelected(index == selected);
                info.setCollectionItemInfo(AccessibilityNodeInfo.CollectionItemInfo.obtain(0, 1,
                    index, 1, false, index == selected));
            }
        };
        tab.setText(label);
        tab.setGravity(Gravity.CENTER);
        tab.setClickable(true);
        tab.setFocusable(true);
        TypedValue padding = new TypedValue();
        int pixels = (int) TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, 12,
            getResources().getDisplayMetrics());
        tab.setPadding(pixels, pixels, pixels, pixels);
        TypedValue background = new TypedValue();
        if (getContext().getTheme().resolveAttribute(android.R.attr.selectableItemBackground,
            background, true)) {
            tab.setBackgroundResource(background.resourceId);
        }
        TypedValue accent = new TypedValue();
        if (getContext().getTheme().resolveAttribute(android.R.attr.colorAccent, accent, true)) {
            int normal = tab.getCurrentTextColor();
            tab.setTextColor(new ColorStateList(
                new int[][] {new int[] {android.R.attr.state_selected}, new int[0]},
                new int[] {accent.data, normal}));
        }
        tab.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                RnViews.report(tag, Rn.EV_TAB, index, 0, null);
            }
        });
        return tab;
    }

    @Override
    public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
        super.onInitializeAccessibilityNodeInfo(info);
        info.setClassName("android.widget.TabWidget");
        info.setCollectionInfo(AccessibilityNodeInfo.CollectionInfo.obtain(1, getChildCount(),
            false, AccessibilityNodeInfo.CollectionInfo.SELECTION_MODE_SINGLE));
    }
}
