package dev.rustnative.android;

import android.app.Activity;
import android.app.DatePickerDialog;
import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Paint;
import android.text.Editable;
import android.text.InputType;
import android.text.TextWatcher;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.widget.AdapterView;
import android.widget.ArrayAdapter;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.CompoundButton;
import android.widget.DatePicker;
import android.widget.EditText;
import android.widget.HorizontalScrollView;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ListView;
import android.widget.ProgressBar;
import android.widget.RadioButton;
import android.widget.ScrollView;
import android.widget.SeekBar;
import android.widget.Spinner;
import android.widget.Switch;
import android.widget.TextView;
import java.nio.ByteBuffer;

/**
 * Creates the platform widget each node kind is realized as, wires its
 * listeners to {@link RnBridge#nativeViewEvent}, and applies the changes
 * Rust makes to it. Every method runs on the main thread, and Rust mutes
 * listeners ({@link RnBridge#muted}) around its own changes.
 */
final class RnViews {
    private RnViews() {}

    /** What every realized view carries: its window, its tag, and its kind. */
    static final class Tag {
        final long window;
        final int tag;
        final int kind;
        /** The selection Rust last set, for widgets that report it late. */
        int selected = -1;
        /** A numeric value Rust last set (a stepper's), and its range. */
        long value;
        long min;
        long max;
        /** The date Rust last set, as yyyymmdd. */
        int date;

        Tag(long window, int tag, int kind) {
            this.window = window;
            this.tag = tag;
            this.kind = kind;
        }
    }

    static Tag tagOf(View view) {
        Object tag = view.getTag();
        return tag instanceof Tag ? (Tag) tag : null;
    }

    static void report(Tag tag, int event, long a, long b, String text) {
        if (!RnBridge.muted) {
            RnBridge.nativeViewEvent(tag.window, tag.tag, event, a, b, text);
        }
    }

    // ---- Creation. ----

    /** Creates the view for {@code kind}; {@code argument} is kind-specific (a scroll's axes). */
    static View create(final Activity activity, int kind, long window, int tagNumber, int argument) {
        final Tag tag = new Tag(window, tagNumber, kind);
        View view;
        switch (kind) {
            case Rn.LABEL:
                view = new TextView(activity);
                break;
            case Rn.BUTTON:
                view = clicks(new Button(activity), tag);
                break;
            case Rn.TEXT_INPUT:
            case Rn.PASSWORD:
            case Rn.MULTILINE:
                view = editText(activity, kind, tag);
                break;
            case Rn.CONTAINER:
                view = new RnLayout(activity);
                break;
            case Rn.SCROLL:
                view = scroll(activity, argument, tag);
                break;
            case Rn.TAB_BAR:
                view = new RnTabs(activity, tag);
                break;
            case Rn.CHECKBOX:
                view = toggles(new CheckBox(activity), tag);
                break;
            case Rn.TOGGLE:
                view = toggles(new Switch(activity), tag);
                break;
            case Rn.RADIO:
                final RadioButton radio = new RadioButton(activity);
                radio.setOnClickListener(new View.OnClickListener() {
                    @Override
                    public void onClick(View v) {
                        report(tag, Rn.EV_TOGGLED, 1, 0, null);
                    }
                });
                view = radio;
                break;
            case Rn.SLIDER:
                SeekBar seek = new SeekBar(activity);
                seek.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener() {
                    @Override
                    public void onProgressChanged(SeekBar bar, int progress, boolean fromUser) {
                        if (fromUser) {
                            report(tag, Rn.EV_VALUE, progress, 0, null);
                        }
                    }

                    @Override
                    public void onStartTrackingTouch(SeekBar bar) {}

                    @Override
                    public void onStopTrackingTouch(SeekBar bar) {}
                });
                view = seek;
                break;
            case Rn.PROGRESS:
                view = new ProgressBar(activity, null, android.R.attr.progressBarStyleHorizontal);
                break;
            case Rn.SELECT:
                Spinner spinner = new Spinner(activity, Spinner.MODE_DROPDOWN);
                spinner.setAdapter(new ArrayAdapter<String>(activity,
                    android.R.layout.simple_spinner_item));
                ((ArrayAdapter<?>) spinner.getAdapter())
                    .setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);
                spinner.setOnItemSelectedListener(new AdapterView.OnItemSelectedListener() {
                    @Override
                    public void onItemSelected(AdapterView<?> parent, View v, int position, long id) {
                        // The spinner reports its first selection after
                        // layout, long after Rust set it: only a change from
                        // what Rust set is the person's.
                        if (position != tag.selected) {
                            tag.selected = position;
                            report(tag, Rn.EV_SELECTION, position, 0, null);
                        }
                    }

                    @Override
                    public void onNothingSelected(AdapterView<?> parent) {}
                });
                view = spinner;
                break;
            case Rn.LIST_BOX:
                ListView list = new ListView(activity);
                list.setChoiceMode(ListView.CHOICE_MODE_SINGLE);
                list.setAdapter(new ArrayAdapter<String>(activity,
                    android.R.layout.simple_list_item_single_choice));
                list.setOnItemClickListener(new AdapterView.OnItemClickListener() {
                    @Override
                    public void onItemClick(AdapterView<?> parent, View v, int position, long id) {
                        report(tag, Rn.EV_SELECTION, position, 0, null);
                    }
                });
                view = list;
                break;
            case Rn.DATE:
                final Button date = new Button(activity);
                date.setOnClickListener(new View.OnClickListener() {
                    @Override
                    public void onClick(View v) {
                        int current = tag.date;
                        DatePickerDialog dialog = new DatePickerDialog(activity,
                            new DatePickerDialog.OnDateSetListener() {
                                @Override
                                public void onDateSet(DatePicker picker, int y, int m, int d) {
                                    report(tag, Rn.EV_DATE, y * 10000L + (m + 1) * 100L + d, 0,
                                        null);
                                }
                            }, current / 10000, (current / 100) % 100 - 1, current % 100);
                        dialog.show();
                    }
                });
                view = date;
                break;
            case Rn.SPINNER:
                view = new RnStepper(activity, tag);
                break;
            case Rn.SEPARATOR:
                View rule = new View(activity);
                TypedValue divider = new TypedValue();
                if (activity.getTheme().resolveAttribute(android.R.attr.listDivider, divider, true)) {
                    rule.setBackgroundResource(divider.resourceId);
                }
                view = rule;
                break;
            case Rn.LINK:
                TextView link = new TextView(activity);
                link.setPaintFlags(link.getPaintFlags() | Paint.UNDERLINE_TEXT_FLAG);
                link.setTextColor(link.getLinkTextColors());
                link.setClickable(true);
                link.setFocusable(true);
                view = clicks(link, tag);
                break;
            case Rn.IMAGE:
                ImageView image = new ImageView(activity);
                image.setScaleType(ImageView.ScaleType.FIT_CENTER);
                view = image;
                break;
            case Rn.CANVAS:
                view = new RnCanvasView(activity, tag);
                break;
            case Rn.SURFACE:
                view = new RnSurfaceView(activity, tag);
                break;
            default:
                view = new View(activity);
                break;
        }
        view.setTag(tag);
        if (kind != Rn.CONTAINER && kind != Rn.SCROLL) {
            view.setOnFocusChangeListener(new View.OnFocusChangeListener() {
                @Override
                public void onFocusChange(View v, boolean focused) {
                    report(tag, focused ? Rn.EV_FOCUS : Rn.EV_BLUR, 0, 0, null);
                }
            });
        }
        return view;
    }

    private static View clicks(View view, final Tag tag) {
        view.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                report(tag, Rn.EV_CLICK, 0, 0, null);
            }
        });
        return view;
    }

    private static View toggles(CompoundButton button, final Tag tag) {
        button.setOnCheckedChangeListener(new CompoundButton.OnCheckedChangeListener() {
            @Override
            public void onCheckedChanged(CompoundButton b, boolean checked) {
                report(tag, Rn.EV_TOGGLED, checked ? 1 : 0, 0, null);
            }
        });
        return button;
    }

    private static View editText(Context context, int kind, final Tag tag) {
        EditText edit = new EditText(context);
        if (kind == Rn.MULTILINE) {
            edit.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE);
            edit.setGravity(Gravity.TOP | Gravity.START);
            edit.setSingleLine(false);
        } else {
            edit.setSingleLine(true);
            if (kind == Rn.PASSWORD) {
                edit.setInputType(InputType.TYPE_CLASS_TEXT
                    | InputType.TYPE_TEXT_VARIATION_PASSWORD);
            }
        }
        edit.addTextChangedListener(new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence s, int start, int count, int after) {}

            @Override
            public void onTextChanged(CharSequence s, int start, int before, int count) {}

            @Override
            public void afterTextChanged(Editable s) {
                report(tag, Rn.EV_TEXT, 0, 0, s.toString());
            }
        });
        return edit;
    }

    private static View scroll(Context context, int axes, final Tag tag) {
        final RnLayout content = new RnLayout(context);
        View.OnScrollChangeListener listener = new View.OnScrollChangeListener() {
            @Override
            public void onScrollChange(View v, int x, int y, int oldX, int oldY) {
                int[] offset = scrollOffset(scrollRoot(v));
                report(tag, Rn.EV_SCROLL, offset[0], offset[1], null);
            }
        };
        switch (axes) {
            case 1: {
                HorizontalScrollView horizontal = new HorizontalScrollView(context);
                horizontal.setFillViewport(true);
                horizontal.addView(content, new ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT));
                horizontal.setOnScrollChangeListener(listener);
                return horizontal;
            }
            case 3: {
                ScrollView vertical = new ScrollView(context);
                vertical.setFillViewport(true);
                HorizontalScrollView horizontal = new HorizontalScrollView(context);
                horizontal.setFillViewport(true);
                horizontal.addView(content, new ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT));
                vertical.addView(horizontal, new ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT));
                vertical.setOnScrollChangeListener(listener);
                horizontal.setOnScrollChangeListener(listener);
                return vertical;
            }
            default: {
                ScrollView vertical = new ScrollView(context);
                vertical.setFillViewport(true);
                vertical.addView(content, new ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
                vertical.setOnScrollChangeListener(listener);
                return vertical;
            }
        }
    }

    /** The outermost scroll view of a scroll container, from any of its parts. */
    static View scrollRoot(View view) {
        if (view instanceof HorizontalScrollView && view.getParent() instanceof ScrollView) {
            return (View) view.getParent();
        }
        return view;
    }

    /** The layout a scroll container's children go into. */
    static RnLayout scrollContent(View scroll) {
        View inner = ((ViewGroup) scroll).getChildAt(0);
        if (inner instanceof HorizontalScrollView) {
            inner = ((ViewGroup) inner).getChildAt(0);
        }
        return (RnLayout) inner;
    }

    /** Where a scroll container is scrolled to, as x and y. */
    static int[] scrollOffset(View scroll) {
        View inner = ((ViewGroup) scroll).getChildAt(0);
        if (inner instanceof HorizontalScrollView) {
            return new int[] {inner.getScrollX(), scroll.getScrollY()};
        }
        return new int[] {scroll.getScrollX(), scroll.getScrollY()};
    }

    /** Scrolls a scroll container to {@code (x, y)}. */
    static void scrollTo(View scroll, int x, int y) {
        View inner = ((ViewGroup) scroll).getChildAt(0);
        if (inner instanceof HorizontalScrollView) {
            inner.scrollTo(x, 0);
            scroll.scrollTo(0, y);
        } else {
            scroll.scrollTo(x, y);
        }
    }


    // ---- Hit testing (input). ----

    /**
     * The deepest realized view under {@code (x, y)} in {@code root}'s
     * pixels: its tag and the point in its own pixels, or tag -1.
     */
    static int[] hitTest(View root, float x, float y) {
        int[] found = deepest(root, x, y);
        return found != null ? found : new int[] {-1, 0, 0};
    }

    private static int[] deepest(View view, float x, float y) {
        if (view.getVisibility() != View.VISIBLE || x < 0 || y < 0 || x >= view.getWidth()
            || y >= view.getHeight()) {
            return null;
        }
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int i = group.getChildCount() - 1; i >= 0; i--) {
                View child = group.getChildAt(i);
                float childX = x + group.getScrollX() - child.getLeft() - child.getTranslationX();
                float childY = y + group.getScrollY() - child.getTop() - child.getTranslationY();
                int[] found = deepest(child, childX, childY);
                if (found != null) {
                    return found;
                }
            }
        }
        Tag tag = tagOf(view);
        if (tag != null && tag.tag >= 0) {
            return new int[] {tag.tag, Math.round(x), Math.round(y)};
        }
        return null;
    }

    /** {@code (x, y)} in {@code root}'s pixels, in {@code view}'s pixels. */
    static int[] toLocal(View root, View view, float x, float y) {
        float localX = x;
        float localY = y;
        View current = view;
        while (current != null && current != root) {
            localX -= current.getLeft() + current.getTranslationX();
            localY -= current.getTop() + current.getTranslationY();
            if (!(current.getParent() instanceof View)) {
                break;
            }
            View parent = (View) current.getParent();
            localX += parent.getScrollX();
            localY += parent.getScrollY();
            current = parent;
        }
        return new int[] {Math.round(localX), Math.round(localY)};
    }

    /** Stops every ancestor of {@code view} intercepting the current touch sequence. */
    static void claimSequence(View view) {
        if (view.getParent() != null) {
            view.getParent().requestDisallowInterceptTouchEvent(true);
        }
    }

    // ---- Drag and drop. ----

    /**
     * Makes {@code view} a drop target (or not). Each step of a drag over it
     * is reported as {@code Rn.EV_DRAG} with {@code a} 1 enter, 2 over,
     * 3 leave, 4 drop, the position packed in {@code b}, and the data as
     * text then URIs, separated by U+0001.
     */
    /** The pointer icon a hovering mouse or stylus shows (0: the view's own). */
    static void setCursor(View view, int type) {
        if (android.os.Build.VERSION.SDK_INT >= 24) {
            view.setPointerIcon(type == 0 ? null
                : android.view.PointerIcon.getSystemIcon(view.getContext(), type));
        }
    }

    /** Whether the view shows system pointer icon {@code type} (tests read it). */
    static boolean showsCursor(View view, int type) {
        return android.os.Build.VERSION.SDK_INT >= 24 && view.getPointerIcon() != null
            && view.getPointerIcon().equals(android.view.PointerIcon.getSystemIcon(view.getContext(), type));
    }

    static void setDropTarget(final View view, boolean on) {
        final Tag tag = tagOf(view);
        if (!on || tag == null) {
            view.setOnDragListener(null);
            return;
        }
        view.setOnDragListener(new View.OnDragListener() {
            @Override
            public boolean onDrag(View v, android.view.DragEvent event) {
                long position = ((long) Math.round(event.getX()) << 32)
                    | (Math.round(event.getY()) & 0xffffffffL);
                switch (event.getAction()) {
                    case android.view.DragEvent.ACTION_DRAG_STARTED:
                        return true;
                    case android.view.DragEvent.ACTION_DRAG_ENTERED:
                        RnBridge.nativeViewEvent(tag.window, tag.tag, Rn.EV_DRAG, 1, position, null);
                        return true;
                    case android.view.DragEvent.ACTION_DRAG_LOCATION:
                        RnBridge.nativeViewEvent(tag.window, tag.tag, Rn.EV_DRAG, 2, position, null);
                        return true;
                    case android.view.DragEvent.ACTION_DRAG_EXITED:
                        RnBridge.nativeViewEvent(tag.window, tag.tag, Rn.EV_DRAG, 3, 0, null);
                        return true;
                    case android.view.DragEvent.ACTION_DROP: {
                        if (v.getContext() instanceof Activity) {
                            ((Activity) v.getContext()).requestDragAndDropPermissions(event);
                        }
                        RnBridge.nativeViewEvent(tag.window, tag.tag, Rn.EV_DRAG, 4, position,
                            describe(event.getClipData(), v.getContext()));
                        return true;
                    }
                    default:
                        return true;
                }
            }
        });
    }

    /** Dropped data as text then URIs, separated by U+0001. */
    static String describe(android.content.ClipData clip, Context context) {
        StringBuilder text = new StringBuilder();
        StringBuilder uris = new StringBuilder();
        if (clip != null) {
            for (int i = 0; i < clip.getItemCount(); i++) {
                android.content.ClipData.Item item = clip.getItemAt(i);
                if (item.getUri() != null) {
                    uris.append('\u0001').append(item.getUri());
                } else if (item.getText() != null) {
                    text.append(item.coerceToText(context));
                }
            }
        }
        return text.toString() + uris;
    }

    // ---- Changes. ----

    static void setText(View view, String text) {
        if (view instanceof RnStepper) {
            return;
        }
        if (view instanceof TextView) {
            TextView textView = (TextView) view;
            if (!textView.getText().toString().equals(text)) {
                if (view instanceof EditText) {
                    EditText edit = (EditText) view;
                    int at = Math.min(edit.getSelectionEnd(), text.length());
                    edit.setText(text);
                    edit.setSelection(Math.max(0, at));
                } else {
                    textView.setText(text);
                }
            }
        }
    }

    static void setHint(View view, String hint) {
        if (view instanceof TextView) {
            ((TextView) view).setHint(hint);
        }
    }

    static void setChecked(View view, boolean checked) {
        if (view instanceof CompoundButton && ((CompoundButton) view).isChecked() != checked) {
            ((CompoundButton) view).setChecked(checked);
        }
    }

    static void setRange(View view, long min, long max, long value) {
        if (view instanceof SeekBar) {
            SeekBar seek = (SeekBar) view;
            int low = (int) Math.max(Integer.MIN_VALUE, min);
            int high = (int) Math.min(Integer.MAX_VALUE, max);
            if (seek.getMin() != low) {
                seek.setMin(low);
            }
            if (seek.getMax() != high) {
                seek.setMax(high);
            }
            seek.setProgress((int) Math.max(low, Math.min(high, value)));
        } else if (view instanceof RnStepper) {
            ((RnStepper) view).set(min, max, value);
        }
    }

    static void setProgress(View view, int percent) {
        ProgressBar bar = (ProgressBar) view;
        if (percent < 0) {
            bar.setIndeterminate(true);
        } else {
            bar.setIndeterminate(false);
            bar.setMax(100);
            bar.setProgress(percent);
        }
    }

    @SuppressWarnings("unchecked")
    static void setOptions(View view, String[] options, int selected) {
        Tag tag = tagOf(view);
        if (view instanceof RnTabs) {
            ((RnTabs) view).set(options, selected);
            return;
        }
        AdapterView<?> adapterView = (AdapterView<?>) view;
        ArrayAdapter<String> adapter = (ArrayAdapter<String>) adapterView.getAdapter();
        boolean same = adapter.getCount() == options.length;
        for (int i = 0; same && i < options.length; i++) {
            same = options[i].equals(adapter.getItem(i));
        }
        if (!same) {
            adapter.clear();
            adapter.addAll(options);
            adapter.notifyDataSetChanged();
        }
        if (tag != null) {
            tag.selected = selected;
        }
        if (view instanceof Spinner) {
            if (selected >= 0 && ((Spinner) view).getSelectedItemPosition() != selected) {
                ((Spinner) view).setSelection(selected, false);
            }
        } else if (view instanceof ListView) {
            ListView list = (ListView) view;
            if (selected >= 0) {
                list.setItemChecked(selected, true);
            } else {
                list.clearChoices();
                list.requestLayout();
            }
        }
    }

    static void setDate(View view, int year, int month, int day, String label) {
        Tag tag = tagOf(view);
        if (tag != null) {
            tag.date = year * 10000 + month * 100 + day;
        }
        setText(view, label);
    }

    /** Shows {@code width} × {@code height} premultiplied RGBA pixels. */
    static void setImage(View view, int width, int height, byte[] pixels) {
        if (width <= 0 || height <= 0) {
            ((ImageView) view).setImageDrawable(null);
            return;
        }
        Bitmap bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888);
        bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(pixels));
        ((ImageView) view).setImageBitmap(bitmap);
    }

    static void setEnabled(View view, boolean enabled) {
        if (view.isEnabled() != enabled) {
            view.setEnabled(enabled);
            if (view instanceof ViewGroup && !(view instanceof RnLayout)) {
                ViewGroup group = (ViewGroup) view;
                for (int i = 0; i < group.getChildCount(); i++) {
                    group.getChildAt(i).setEnabled(enabled);
                }
            }
        }
    }

    static void setVisible(View view, boolean visible) {
        int visibility = visible ? View.VISIBLE : View.GONE;
        if (view.getVisibility() != visibility) {
            view.setVisibility(visibility);
        }
    }

    static void setAlpha(View view, float alpha) {
        if (view.getAlpha() != alpha) {
            view.setAlpha(alpha);
        }
    }

    /** Whether {@code view} has input focus. */
    static boolean hasFocus(View view) {
        return view.isFocused();
    }

    /** Gives {@code view} input focus. */
    static void focus(View view) {
        if (!view.isFocusable()) {
            view.setFocusable(true);
        }
        view.requestFocus();
    }

    static void setFocusable(View view, boolean focusable) {
        if (view.isFocusable() != focusable) {
            view.setFocusable(focusable);
            view.setFocusableInTouchMode(focusable && !(view instanceof RnLayout));
        }
    }

    static void setDirection(View view, boolean rightToLeft) {
        int direction = rightToLeft ? View.LAYOUT_DIRECTION_RTL : View.LAYOUT_DIRECTION_LTR;
        if (view.getLayoutDirection() != direction) {
            view.setLayoutDirection(direction);
        }
    }

    /** The view's rectangle in its parent, in pixels: left, top, width, height. */
    static int[] frame(View view) {
        return new int[] {view.getLeft(), view.getTop(), view.getWidth(), view.getHeight()};
    }

    /** The view's position on screen, in pixels. */
    static int[] screenPosition(View view) {
        int[] at = new int[2];
        view.getLocationOnScreen(at);
        return at;
    }

    /** The class name of a view, for inspection and tests. */
    static String className(View view) {
        return view.getClass().getName();
    }

    /** A view's text, for tests. */
    /** Sets a view's tooltip (a mapper's example; API 26+). */
    static void setTooltip(View view, String text) {
        view.setTooltipText(text);
    }

    /** A view's tooltip (tests read it). */
    static String tooltip(View view) {
        CharSequence text = view.getTooltipText();
        return text == null ? null : text.toString();
    }

    static String text(View view) {
        return view instanceof TextView ? ((TextView) view).getText().toString() : null;
    }

    /** Lays the view's subtree out now (tests and the inspector read geometry). */
    static void layoutNow(View view) {
        View root = view.getRootView();
        root.measure(View.MeasureSpec.makeMeasureSpec(root.getWidth(), View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(root.getHeight(), View.MeasureSpec.EXACTLY));
        root.layout(root.getLeft(), root.getTop(), root.getRight(), root.getBottom());
    }

    /** A plain linear row, used by composite controls. */
    static LinearLayout row(Context context) {
        LinearLayout row = new LinearLayout(context);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        return row;
    }
}
