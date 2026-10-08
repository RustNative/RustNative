package dev.rustnative.android;

import android.view.MotionEvent;

/**
 * Flattens a {@link MotionEvent} for Rust, which translates it into the
 * portable pointer model ({@code input::pointer}).
 */
final class RnPointer {
    private RnPointer() {}

    /** Floats per pointer: x, y, pressure, tilt, orientation, h-scroll, v-scroll. */
    static final int VALUES = 7;

    /** Offers {@code event} to the framework; returns whether it claimed it. */
    static boolean offer(long window, MotionEvent event) {
        int count = event.getPointerCount();
        int[] ids = new int[count];
        int[] tools = new int[count];
        float[] values = new float[count * VALUES];
        for (int i = 0; i < count; i++) {
            ids[i] = event.getPointerId(i);
            tools[i] = event.getToolType(i);
            int at = i * VALUES;
            values[at] = event.getX(i);
            values[at + 1] = event.getY(i);
            values[at + 2] = event.getPressure(i);
            values[at + 3] = event.getAxisValue(MotionEvent.AXIS_TILT, i);
            values[at + 4] = event.getOrientation(i);
            values[at + 5] = event.getAxisValue(MotionEvent.AXIS_HSCROLL, i);
            values[at + 6] = event.getAxisValue(MotionEvent.AXIS_VSCROLL, i);
        }
        return RnBridge.nativePointer(window, event.getActionMasked(), event.getActionIndex(), ids,
            tools, values, event.getButtonState(), event.getMetaState(), event.getEventTime());
    }
}
