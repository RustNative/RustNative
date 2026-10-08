package dev.rustnative.android;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Matrix;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.RectF;
import android.text.Layout;
import android.text.StaticLayout;
import android.text.TextPaint;
import android.view.MotionEvent;
import android.view.View;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;

/**
 * A canvas node: replays the draw list Rust encoded ({@code canvas.rs}) in
 * {@link #onDraw}, in canvas units (dp) scaled to the display's density.
 * Text goes through {@link StaticLayout}, the host's own shaping and line
 * breaking.
 */
final class RnCanvasView extends View {
    // Opcodes (`canvas.rs`'s `op`).
    static final int FILL_RECT = 1;
    static final int STROKE_RECT = 2;
    static final int FILL_ROUNDED_RECT = 3;
    static final int FILL_ELLIPSE = 4;
    static final int STROKE_LINE = 5;
    static final int FILL_PATH = 6;
    static final int STROKE_PATH = 7;
    static final int TEXT = 8;
    static final int IMAGE = 9;
    static final int PUSH_TRANSFORM = 10;
    static final int PUSH_CLIP = 11;
    static final int PUSH_OPACITY = 12;
    static final int POP = 13;

    final RnViews.Tag tag;
    private byte[] commands = new byte[0];
    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final TextPaint text = new TextPaint(Paint.ANTI_ALIAS_FLAG);
    private final RectF rect = new RectF();
    private final Path path = new Path();

    RnCanvasView(Context context, RnViews.Tag tag) {
        super(context);
        this.tag = tag;
    }

    /** Replaces the draw list (Rust calls it). */
    static void setCommands(View view, byte[] commands) {
        RnCanvasView canvas = (RnCanvasView) view;
        canvas.commands = commands;
        canvas.invalidate();
    }

    /** Draws the commands into a bitmap of the view's size (tests read it back). */
    static int[] render(View view, int width, int height) {
        Bitmap bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888);
        ((RnCanvasView) view).replay(new Canvas(bitmap));
        int[] pixels = new int[width * height];
        bitmap.getPixels(pixels, 0, width, 0, 0, width, height);
        return pixels;
    }

    @Override
    protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        replay(canvas);
    }

    private void replay(Canvas canvas) {
        float density = getResources().getDisplayMetrics().density;
        int base = canvas.save();
        canvas.scale(density, density);
        ByteBuffer in = ByteBuffer.wrap(commands).order(ByteOrder.LITTLE_ENDIAN);
        while (in.hasRemaining()) {
            int op = in.get() & 0xff;
            switch (op) {
                case FILL_RECT:
                    readRect(in);
                    fill(in.getInt());
                    canvas.drawRect(rect, paint);
                    break;
                case STROKE_RECT:
                    readRect(in);
                    stroke(in.getInt(), in.getFloat());
                    canvas.drawRect(rect, paint);
                    break;
                case FILL_ROUNDED_RECT: {
                    readRect(in);
                    float radius = in.getFloat();
                    fill(in.getInt());
                    canvas.drawRoundRect(rect, radius, radius, paint);
                    break;
                }
                case FILL_ELLIPSE:
                    readRect(in);
                    fill(in.getInt());
                    canvas.drawOval(rect, paint);
                    break;
                case STROKE_LINE: {
                    float x1 = in.getFloat();
                    float y1 = in.getFloat();
                    float x2 = in.getFloat();
                    float y2 = in.getFloat();
                    stroke(in.getInt(), in.getFloat());
                    canvas.drawLine(x1, y1, x2, y2, paint);
                    break;
                }
                case FILL_PATH:
                    readPath(in);
                    fill(in.getInt());
                    path.setFillType(Path.FillType.WINDING);
                    canvas.drawPath(path, paint);
                    break;
                case STROKE_PATH:
                    readPath(in);
                    stroke(in.getInt(), in.getFloat());
                    canvas.drawPath(path, paint);
                    break;
                case TEXT: {
                    float x = in.getFloat();
                    float y = in.getFloat();
                    float size = in.getFloat();
                    int color = in.getInt();
                    byte[] bytes = new byte[in.getInt()];
                    in.get(bytes);
                    String string = new String(bytes, StandardCharsets.UTF_8);
                    text.setTextSize(size);
                    text.setColor(color);
                    int width = (int) Math.ceil(Layout.getDesiredWidth(string, text));
                    StaticLayout layout = StaticLayout.Builder.obtain(string, 0, string.length(), text,
                        Math.max(1, width)).build();
                    canvas.save();
                    canvas.translate(x, y);
                    layout.draw(canvas);
                    canvas.restore();
                    break;
                }
                case IMAGE: {
                    readRect(in);
                    int width = in.getInt();
                    int height = in.getInt();
                    byte[] pixels = new byte[width * height * 4];
                    in.get(pixels);
                    Bitmap bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888);
                    bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(pixels));
                    paint.reset();
                    paint.setFilterBitmap(true);
                    canvas.drawBitmap(bitmap, null, rect, paint);
                    break;
                }
                case PUSH_TRANSFORM: {
                    float[] values = new float[6];
                    for (int i = 0; i < 6; i++) {
                        values[i] = in.getFloat();
                    }
                    Matrix matrix = new Matrix();
                    // Rows: m11 m21 dx / m12 m22 dy (a point is (x, y, 1)).
                    matrix.setValues(new float[] {values[0], values[2], values[4], values[1],
                        values[3], values[5], 0, 0, 1});
                    canvas.save();
                    canvas.concat(matrix);
                    break;
                }
                case PUSH_CLIP:
                    readRect(in);
                    canvas.save();
                    canvas.clipRect(rect);
                    break;
                case PUSH_OPACITY:
                    canvas.saveLayerAlpha(null, Math.round(Math.max(0, Math.min(1, in.getFloat())) * 255));
                    break;
                case POP:
                    if (canvas.getSaveCount() > base + 1) {
                        canvas.restore();
                    }
                    break;
                default:
                    // An opcode this replay does not know: stop rather than
                    // misread what follows.
                    in.position(in.limit());
                    break;
            }
        }
        canvas.restoreToCount(base);
    }

    private void readRect(ByteBuffer in) {
        float x = in.getFloat();
        float y = in.getFloat();
        float width = in.getFloat();
        float height = in.getFloat();
        rect.set(x, y, x + width, y + height);
    }

    private void readPath(ByteBuffer in) {
        path.reset();
        int count = in.getInt();
        for (int i = 0; i < count; i++) {
            switch (in.get()) {
                case 0:
                    path.moveTo(in.getFloat(), in.getFloat());
                    break;
                case 1:
                    path.lineTo(in.getFloat(), in.getFloat());
                    break;
                case 2:
                    path.quadTo(in.getFloat(), in.getFloat(), in.getFloat(), in.getFloat());
                    break;
                case 3:
                    path.cubicTo(in.getFloat(), in.getFloat(), in.getFloat(), in.getFloat(),
                        in.getFloat(), in.getFloat());
                    break;
                default:
                    path.close();
                    break;
            }
        }
    }

    private void fill(int color) {
        paint.reset();
        paint.setAntiAlias(true);
        paint.setStyle(Paint.Style.FILL);
        paint.setColor(color);
    }

    private void stroke(int color, float width) {
        paint.reset();
        paint.setAntiAlias(true);
        paint.setStyle(Paint.Style.STROKE);
        paint.setColor(color);
        paint.setStrokeWidth(width);
    }

    @Override
    public boolean dispatchHoverEvent(MotionEvent event) {
        // A screen reader exploring by touch reaches virtual elements here.
        return RnAccess.dispatchHover(this, event) || super.dispatchHoverEvent(event);
    }

    @Override
    public boolean onCheckIsTextEditor() {
        // A focused framework view is a custom text target (`RnInput`).
        return isFocused() && RnViews.tagOf(this) != null;
    }

    @Override
    public android.view.inputmethod.InputConnection onCreateInputConnection(
        android.view.inputmethod.EditorInfo info) {
        return RnInput.connect(this, info);
    }
}
