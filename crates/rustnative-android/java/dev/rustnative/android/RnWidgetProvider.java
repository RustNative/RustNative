package dev.rustnative.android;

import android.appwidget.AppWidgetManager;
import android.appwidget.AppWidgetProvider;
import android.content.ComponentName;
import android.content.Context;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.widget.RemoteViews;

/**
 * A home-screen widget (`[[android.widgets]]`): its content is what the
 * application last sent (`SurfaceCommand::UpdateWidget`), kept in
 * preferences so the launcher can redraw it while the application is not
 * running. A tap on it, or on one of its actions, opens the application
 * with the action, which arrives as `Event::SurfaceAction`.
 *
 * <p>Each declared widget is one of the slot subclasses, named in the
 * manifest with its id as meta-data ({@code dev.rustnative.surface}).
 */
public class RnWidgetProvider extends AppWidgetProvider {
    public static class Slot0 extends RnWidgetProvider {}
    public static class Slot1 extends RnWidgetProvider {}
    public static class Slot2 extends RnWidgetProvider {}
    public static class Slot3 extends RnWidgetProvider {}

    static final Class<?>[] SLOTS = {Slot0.class, Slot1.class, Slot2.class, Slot3.class};
    private static final String FIELD = "\u0001";
    private static final String ITEM = "\u0002";

    private static SharedPreferences store(Context context) {
        return context.getSharedPreferences("rustnative.widgets", Context.MODE_PRIVATE);
    }

    /** A slot's surface id, from its manifest meta-data (null: undeclared). */
    static String surfaceOf(Context context, ComponentName component, boolean service) {
        try {
            PackageManager manager = context.getPackageManager();
            android.content.pm.PackageItemInfo info = service
                ? manager.getServiceInfo(component, PackageManager.GET_META_DATA)
                : manager.getReceiverInfo(component, PackageManager.GET_META_DATA);
            return info.metaData == null ? null : info.metaData.getString("dev.rustnative.surface");
        } catch (PackageManager.NameNotFoundException e) {
            return null;
        }
    }

    /** Stores widget {@code id}'s content and redraws every placed instance. False: undeclared. */
    static boolean update(Context context, String id, String title, String[] lines, String[] actions) {
        String stored = title + FIELD + String.join(ITEM, lines) + FIELD + String.join(ITEM, actions);
        store(context).edit().putString(id, stored).apply();
        for (Class<?> slot : SLOTS) {
            ComponentName component = new ComponentName(context, slot);
            if (id.equals(surfaceOf(context, component, false))) {
                AppWidgetManager manager = AppWidgetManager.getInstance(context);
                for (int widget : manager.getAppWidgetIds(component)) {
                    manager.updateAppWidget(widget, render(context, id));
                }
                return true;
            }
        }
        return false;
    }

    /** What widget {@code id} shows, as stored (tests read it back). */
    static String content(Context context, String id) {
        return store(context).getString(id, null);
    }

    /** The views widget {@code id} shows. */
    static RemoteViews render(Context context, String id) {
        int layout = context.getResources().getIdentifier("rn_widget", "layout", context.getPackageName());
        RemoteViews views = new RemoteViews(context.getPackageName(), layout);
        int root = android.R.id.background;
        views.removeAllViews(root);
        String stored = content(context, id);
        String[] fields = stored == null ? new String[] {"", "", ""} : stored.split(FIELD, -1);
        views.addView(root, text(context, fields[0], 16));
        if (fields.length > 1 && !fields[1].isEmpty()) {
            for (String line : fields[1].split(ITEM, -1)) {
                views.addView(root, text(context, line, 13));
            }
        }
        if (fields.length > 2 && !fields[2].isEmpty()) {
            String[] actions = fields[2].split(ITEM, -1);
            for (int i = 0; i + 1 < actions.length; i += 2) {
                RemoteViews button = text(context, actions[i + 1], 14);
                button.setOnClickPendingIntent(android.R.id.text1,
                    RnServices.surfaceIntent("widget", id + "/" + actions[i], (id + actions[i]).hashCode()));
                views.addView(root, button);
            }
        }
        views.setOnClickPendingIntent(root, RnServices.surfaceIntent("widget", id, id.hashCode()));
        return views;
    }

    private static RemoteViews text(Context context, String value, float size) {
        RemoteViews line = new RemoteViews(context.getPackageName(), android.R.layout.simple_list_item_1);
        line.setTextViewText(android.R.id.text1, value);
        line.setTextViewTextSize(android.R.id.text1, android.util.TypedValue.COMPLEX_UNIT_SP, size);
        return line;
    }

    @Override
    public void onUpdate(Context context, AppWidgetManager manager, int[] widgets) {
        RnServices.init(context);
        String id = surfaceOf(context, new ComponentName(context, getClass()), false);
        if (id == null) {
            return;
        }
        for (int widget : widgets) {
            manager.updateAppWidget(widget, render(context, id));
        }
    }
}
