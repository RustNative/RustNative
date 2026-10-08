package dev.rustnative.android;

import android.content.Context;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;

/**
 * A native surface node (`surface.rs`): a {@link SurfaceView} whose
 * surface the application renders into with its own GPU code, through the
 * `ANativeWindow` Rust hands out. The framework places it and never paints
 * it. Its surface's life — created, resized, destroyed — is reported as
 * {@code Rn.EV_SURFACE} with {@code a} = 1, 2, or 3 and the size packed in
 * {@code b}.
 */
final class RnSurfaceView extends SurfaceView implements SurfaceHolder.Callback {
    static final int CREATED = 1;
    static final int CHANGED = 2;
    static final int DESTROYED = 3;

    final RnViews.Tag tag;

    RnSurfaceView(Context context, RnViews.Tag tag) {
        super(context);
        this.tag = tag;
        getHolder().addCallback(this);
    }

    /** The surface, while it exists (null otherwise). */
    static Surface surface(android.view.View view) {
        SurfaceHolder holder = ((RnSurfaceView) view).getHolder();
        Surface surface = holder.getSurface();
        return surface != null && surface.isValid() ? surface : null;
    }

    private void report(int what, int width, int height) {
        // Not muted: the surface's life is never an echo of the framework's.
        RnBridge.nativeViewEvent(tag.window, tag.tag, Rn.EV_SURFACE, what,
            ((long) width << 32) | (height & 0xffffffffL), null);
    }

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        report(CREATED, getWidth(), getHeight());
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        report(CHANGED, width, height);
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        // Synchronous: the application must stop rendering before this
        // returns, and Rust releases its window here.
        report(DESTROYED, 0, 0);
    }
}
