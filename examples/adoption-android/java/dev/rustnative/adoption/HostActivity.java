package dev.rustnative.adoption;

import android.app.Activity;
import android.os.Bundle;
import android.widget.LinearLayout;
import android.widget.TextView;
import dev.rustnative.android.RustNativeView;

/**
 * The host application's own activity: its own header, written the way
 * any Android screen is, and below it a {@link RustNativeView} showing the
 * Rust Native application. The host keeps its title, back handling, and
 * lifecycle; the embedded application follows them.
 */
public class HostActivity extends Activity {
    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        LinearLayout column = new LinearLayout(this);
        column.setOrientation(LinearLayout.VERTICAL);
        column.setFitsSystemWindows(true);
        TextView header = new TextView(this);
        header.setText("The host application's own screen");
        header.setTextSize(20);
        header.setPadding(32, 32, 32, 32);
        column.addView(header);
        RustNativeView embedded = new RustNativeView(this);
        column.addView(embedded, new LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f));
        setContentView(column);
    }
}
