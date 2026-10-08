package dev.rustnative.android;

import android.content.ComponentName;
import android.content.Context;
import android.content.SharedPreferences;
import android.os.Build;
import android.service.quicksettings.Tile;
import android.service.quicksettings.TileService;

/**
 * A quick-settings tile (`[[android.tiles]]`): it shows the state the
 * application last sent (`SurfaceCommand::UpdateTile`), kept in preferences
 * so the shade can show it while the application is not running. A tap
 * opens the application with the tile's id as a surface action.
 */
public class RnTileService extends TileService {
    public static class Slot0 extends RnTileService {}
    public static class Slot1 extends RnTileService {}
    public static class Slot2 extends RnTileService {}
    public static class Slot3 extends RnTileService {}

    static final Class<?>[] SLOTS = {Slot0.class, Slot1.class, Slot2.class, Slot3.class};

    private static SharedPreferences store(Context context) {
        return context.getSharedPreferences("rustnative.tiles", Context.MODE_PRIVATE);
    }

    /** Stores tile {@code id}'s state and asks the shade to refresh it. False: undeclared. */
    static boolean update(Context context, String id, String label, String subtitle, boolean active) {
        store(context).edit()
            .putString(id + ".label", label)
            .putString(id + ".subtitle", subtitle)
            .putBoolean(id + ".active", active)
            .apply();
        for (Class<?> slot : SLOTS) {
            ComponentName component = new ComponentName(context, slot);
            if (id.equals(RnWidgetProvider.surfaceOf(context, component, true))) {
                TileService.requestListeningState(context, component);
                return true;
            }
        }
        return false;
    }

    /** Tile {@code id}'s stored state: label, subtitle, and "1" when active (tests read it). */
    static String[] state(Context context, String id) {
        SharedPreferences store = store(context);
        return new String[] {store.getString(id + ".label", null), store.getString(id + ".subtitle", null),
            store.getBoolean(id + ".active", false) ? "1" : "0"};
    }

    private String id() {
        return RnWidgetProvider.surfaceOf(this, new ComponentName(this, getClass()), true);
    }

    @Override
    public void onStartListening() {
        Tile tile = getQsTile();
        String id = id();
        if (tile == null || id == null) {
            return;
        }
        String[] state = state(this, id);
        if (state[0] != null) {
            tile.setLabel(state[0]);
        }
        if (Build.VERSION.SDK_INT >= 29) {
            tile.setSubtitle(state[1]);
        }
        tile.setState("1".equals(state[2]) ? Tile.STATE_ACTIVE : Tile.STATE_INACTIVE);
        tile.updateTile();
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onClick() {
        String id = id();
        if (id == null) {
            return;
        }
        RnServices.init(this);
        android.app.PendingIntent intent = RnServices.surfaceIntent("tile", id, id.hashCode());
        if (Build.VERSION.SDK_INT >= 34) {
            startActivityAndCollapse(intent);
        } else {
            try {
                intent.send();
            } catch (android.app.PendingIntent.CanceledException ignored) {
                // The application is going away.
            }
        }
    }
}
