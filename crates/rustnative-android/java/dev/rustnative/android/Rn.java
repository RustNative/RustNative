package dev.rustnative.android;

/**
 * The numbers Rust and Java agree on. Every constant here has its twin in
 * {@code crates/rustnative-android/src/protocol.rs}; a test there reads this
 * file and holds the two in step.
 */
final class Rn {
    private Rn() {}

    // ---- View kinds: what `RnViews.create` makes. ----
    static final int LABEL = 1;
    static final int BUTTON = 2;
    static final int TEXT_INPUT = 3;
    static final int PASSWORD = 4;
    static final int CONTAINER = 5;
    static final int SCROLL = 6;
    static final int CANVAS = 7;
    static final int SURFACE = 8;
    static final int TAB_BAR = 9;
    static final int CHECKBOX = 11;
    static final int RADIO = 12;
    static final int TOGGLE = 13;
    static final int SLIDER = 14;
    static final int PROGRESS = 15;
    static final int SELECT = 16;
    static final int LIST_BOX = 17;
    static final int DATE = 18;
    static final int SPINNER = 19;
    static final int SEPARATOR = 20;
    static final int LINK = 21;
    static final int MULTILINE = 22;
    static final int IMAGE = 23;
    static final int FOREIGN = 24;
    static final int WEB = 25;
    static final int MEDIA = 26;
    static final int CAMERA = 27;

    // ---- View events: `RnBridge.nativeViewEvent`'s `event`. ----
    static final int EV_CLICK = 1;
    static final int EV_TEXT = 2;
    static final int EV_TOGGLED = 3;
    static final int EV_VALUE = 4;
    static final int EV_SELECTION = 5;
    static final int EV_DATE = 6;
    static final int EV_FOCUS = 7;
    static final int EV_BLUR = 8;
    static final int EV_SCROLL = 9;
    static final int EV_TAB = 10;
    static final int EV_SURFACE = 11;
    static final int EV_PAGE = 12;
    static final int EV_ACCESSIBILITY = 13;
    static final int EV_DRAG = 14;

    // ---- Activity lifecycle: `RnBridge.nativeLifecycle`'s `what`. ----
    static final int LC_START = 1;
    static final int LC_RESUME = 2;
    static final int LC_PAUSE = 3;
    static final int LC_STOP = 4;
    static final int LC_DESTROY = 5;
    static final int LC_DESTROY_FINISHING = 6;
    static final int LC_SAVE = 7;
    static final int LC_CONFIGURATION = 8;
    static final int LC_TRIM = 9;
    static final int LC_LOW_MEMORY = 10;
    static final int LC_MULTI_WINDOW = 11;
    static final int LC_FOCUS = 12;
    static final int LC_POSTURE = 13;

    // ---- Back: `RnBridge.nativeBack`'s `phase`. ----
    static final int BACK_STARTED = 1;
    static final int BACK_PROGRESSED = 2;
    static final int BACK_CANCELLED = 3;
    static final int BACK_INVOKED = 4;

    // ---- Style: the flags of one state's entry in `RnStyle.apply`. ----
    static final int ST_BACKGROUND = 1;
    static final int ST_FOREGROUND = 2;
    static final int ST_BORDER = 4;
    static final int ST_RADIUS = 8;
    static final int ST_ELEVATION = 16;
    /** The states a style has an entry for, in this order. */
    static final int STATE_NORMAL = 0;
    static final int STATE_HOVERED = 1;
    static final int STATE_FOCUSED = 2;
    static final int STATE_PRESSED = 3;
    static final int STATE_DISABLED = 4;
    static final int STATES = 5;
    /** Ints per state: flags, background, foreground, border. */
    static final int STATE_INTS = 4;
    /** Floats per state: radius (px), elevation (px). */
    static final int STATE_FLOATS = 2;
}
