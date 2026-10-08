package dev.rustnative.android;

import android.content.Context;
import android.text.Editable;
import android.text.InputType;
import android.text.TextWatcher;
import android.view.Gravity;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;

/**
 * A whole number stepped up and down: a decrement button, the number, and
 * an increment button. Android has no native spin button outside the
 * time and number pickers' wheels, so this is the composition its own
 * applications use, reported to accessibility services as one control with
 * a range.
 */
final class RnStepper extends LinearLayout {
    private final RnViews.Tag tag;
    private final EditText number;
    private final Button down;
    private final Button up;

    RnStepper(Context context, final RnViews.Tag tag) {
        super(context);
        this.tag = tag;
        setOrientation(HORIZONTAL);
        setGravity(Gravity.CENTER_VERTICAL);
        down = new Button(context);
        down.setText("−");
        down.setContentDescription("Decrease");
        up = new Button(context);
        up.setText("+");
        up.setContentDescription("Increase");
        number = new EditText(context);
        number.setInputType(InputType.TYPE_CLASS_NUMBER | InputType.TYPE_NUMBER_FLAG_SIGNED);
        number.setSingleLine(true);
        number.setGravity(Gravity.CENTER);
        addView(down, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT));
        addView(number, new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f));
        addView(up, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT));
        down.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                step(-1);
            }
        });
        up.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                step(1);
            }
        });
        number.addTextChangedListener(new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence s, int start, int count, int after) {}

            @Override
            public void onTextChanged(CharSequence s, int start, int before, int count) {}

            @Override
            public void afterTextChanged(Editable s) {
                try {
                    long value = Long.parseLong(s.toString());
                    if (value != tag.value && value >= tag.min && value <= tag.max) {
                        RnViews.report(tag, Rn.EV_VALUE, value, 0, null);
                    }
                } catch (NumberFormatException ignored) {
                    // Partial input ("-", empty) reports nothing yet.
                }
            }
        });
    }

    private void step(int by) {
        long value = Math.max(tag.min, Math.min(tag.max, tag.value + by));
        if (value != tag.value) {
            RnViews.report(tag, Rn.EV_VALUE, value, 0, null);
        }
    }

    void set(long min, long max, long value) {
        tag.min = min;
        tag.max = max;
        tag.value = value;
        String text = Long.toString(value);
        if (!number.getText().toString().equals(text)) {
            number.setText(text);
        }
        down.setEnabled(isEnabled() && value > min);
        up.setEnabled(isEnabled() && value < max);
    }

    @Override
    public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
        super.onInitializeAccessibilityNodeInfo(info);
        info.setClassName("android.widget.NumberPicker");
        info.setRangeInfo(AccessibilityNodeInfo.RangeInfo.obtain(
            AccessibilityNodeInfo.RangeInfo.RANGE_TYPE_INT, tag.min, tag.max, tag.value));
    }
}
