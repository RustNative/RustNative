package dev.rustnative.android;

import android.content.Context;
import android.text.InputType;
import android.view.View;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

/**
 * An input method for a custom text target (`input/text.rs`): a focused
 * framework view that is not a native text field — a canvas a text editor
 * draws, say. The soft keyboard's composition and commits arrive as
 * {@link RnBridge#nativeText}; the platform's own text fields keep theirs.
 */
final class RnInput extends BaseInputConnection {
    static final int COMPOSE = 1;
    static final int COMMIT = 2;
    static final int CANCEL = 3;

    private final View view;
    private final RnViews.Tag tag;
    private boolean composing;

    RnInput(View view, RnViews.Tag tag) {
        super(view, false);
        this.view = view;
        this.tag = tag;
    }

    /** The connection a framework view offers while it is a text target. */
    static InputConnection connect(View view, EditorInfo info) {
        RnViews.Tag tag = RnViews.tagOf(view);
        if (tag == null || !view.isFocused()) {
            return null;
        }
        info.inputType = InputType.TYPE_CLASS_TEXT;
        info.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN;
        return new RnInput(view, tag);
    }

    /** Shows the soft keyboard for {@code view} (Rust calls it on focus). */
    static void show(View view) {
        InputMethodManager manager = (InputMethodManager)
            view.getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
        if (manager != null) {
            view.requestFocus();
            manager.restartInput(view);
            manager.showSoftInput(view, 0);
        }
    }

    /** Hides the soft keyboard. */
    static void hide(View view) {
        InputMethodManager manager = (InputMethodManager)
            view.getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
        if (manager != null) {
            manager.hideSoftInputFromWindow(view.getWindowToken(), 0);
        }
    }

    @Override
    public boolean setComposingText(CharSequence text, int newCursorPosition) {
        composing = true;
        RnBridge.nativeText(tag.window, tag.tag, COMPOSE, text.toString(),
            Math.max(0, Math.min(text.length(), newCursorPosition > 0 ? text.length() : 0)));
        return true;
    }

    @Override
    public boolean commitText(CharSequence text, int newCursorPosition) {
        composing = false;
        RnBridge.nativeText(tag.window, tag.tag, COMMIT, text.toString(), text.length());
        return true;
    }

    @Override
    public boolean finishComposingText() {
        if (composing) {
            composing = false;
            RnBridge.nativeText(tag.window, tag.tag, CANCEL, "", 0);
        }
        return true;
    }

    @Override
    public boolean deleteSurroundingText(int before, int after) {
        // A backspace with nothing composing: the key the target hears.
        for (int i = 0; i < before; i++) {
            view.dispatchKeyEvent(new android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN,
                android.view.KeyEvent.KEYCODE_DEL));
            view.dispatchKeyEvent(new android.view.KeyEvent(android.view.KeyEvent.ACTION_UP,
                android.view.KeyEvent.KEYCODE_DEL));
        }
        return true;
    }
}
