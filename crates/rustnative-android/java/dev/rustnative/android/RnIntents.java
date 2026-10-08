package dev.rustnative.android;

import android.content.ClipData;
import android.content.Intent;
import android.net.Uri;
import android.os.Build;
import java.util.ArrayList;

/**
 * Reads what an intent brings the application ({@code intents.rs}): a deep
 * link, shared content, or a tap on one of its surfaces.
 */
final class RnIntents {
    private RnIntents() {}

    /** The extra naming the surface a tap came from. */
    static final String EXTRA_SURFACE = "dev.rustnative.surface";
    /** The extra naming the action a tap chose. */
    static final String EXTRA_ACTION = "dev.rustnative.action";

    /**
     * {@code ["view", url]}, {@code ["send", text, subject, uri, type, uri,
     * type, ...]}, {@code ["surface", surface, action]}, or {@code [""]}.
     */
    @SuppressWarnings("deprecation")
    static String[] describe(Intent intent) {
        if (intent == null) {
            return new String[] {""};
        }
        String surface = intent.getStringExtra(EXTRA_SURFACE);
        if (surface != null) {
            String action = intent.getStringExtra(EXTRA_ACTION);
            return new String[] {"surface", surface, action == null ? "activate" : action};
        }
        String action = intent.getAction();
        if (Intent.ACTION_VIEW.equals(action) && intent.getData() != null) {
            return new String[] {"view", intent.getData().toString()};
        }
        if (Intent.ACTION_SEND.equals(action) || Intent.ACTION_SEND_MULTIPLE.equals(action)) {
            ArrayList<String> out = new ArrayList<>();
            out.add("send");
            CharSequence text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT);
            out.add(text == null ? null : text.toString());
            out.add(intent.getStringExtra(Intent.EXTRA_SUBJECT));
            String type = intent.getType() == null ? "application/octet-stream" : intent.getType();
            ClipData clip = intent.getClipData();
            if (clip != null) {
                for (int i = 0; i < clip.getItemCount(); i++) {
                    Uri uri = clip.getItemAt(i).getUri();
                    if (uri != null) {
                        out.add(uri.toString());
                        out.add(type);
                    }
                }
            } else {
                Uri stream = intent.getParcelableExtra(Intent.EXTRA_STREAM);
                if (stream != null) {
                    out.add(stream.toString());
                    out.add(type);
                }
            }
            return out.toArray(new String[0]);
        }
        return new String[] {""};
    }

    /** Whether the intent marks a test run (the device suite's activity). */
    static boolean isTest(Intent intent) {
        return intent != null && intent.getBooleanExtra("dev.rustnative.test", false);
    }
}
