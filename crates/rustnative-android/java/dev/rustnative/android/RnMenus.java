package dev.rustnative.android;

import android.app.Activity;
import android.view.Menu;
import android.view.MenuItem;
import android.view.SubMenu;
import android.view.View;
import android.widget.PopupMenu;

/**
 * A window's menu bar as its activity's options menu, and context menus as
 * popup menus. Rust describes the menu as a flat list in which each entry
 * names its parent (submenus), so this class only rebuilds what it is told.
 */
final class RnMenus {
    private RnMenus() {}

    /** The tag a menu choice is reported with (`nativeViewEvent`). */
    static final int MENU_TAG = -1;

    /** A menu, flattened: entry {@code i}'s parent is {@code parents[i]} (-1 at the top). */
    static final class Model {
        String[] ids;
        String[] labels;
        int[] parents;
        /** Bit 0 enabled, bit 1 checkable, bit 2 checked, bit 3 submenu, bit 4 separator. */
        int[] flags;
        String[] shortcuts;
    }

    /** Sets {@code activity}'s options menu (Rust calls it). */
    static void set(RnActivity activity, String[] ids, String[] labels, int[] parents, int[] flags,
        String[] shortcuts) {
        Model model = new Model();
        model.ids = ids;
        model.labels = labels;
        model.parents = parents;
        model.flags = flags;
        model.shortcuts = shortcuts;
        activity.menu = model;
        activity.invalidateOptionsMenu();
    }

    static boolean populate(RnActivity activity, Menu menu) {
        menu.clear();
        Model model = activity.menu;
        if (model == null) {
            return false;
        }
        Menu[] containers = new Menu[model.ids.length];
        for (int i = 0; i < model.ids.length; i++) {
            Menu parent = model.parents[i] < 0 ? menu : containers[model.parents[i]];
            if (parent == null || (model.flags[i] & 16) != 0) {
                continue;
            }
            if ((model.flags[i] & 8) != 0) {
                SubMenu sub = parent.addSubMenu(Menu.NONE, i, i, model.labels[i]);
                containers[i] = sub;
                continue;
            }
            MenuItem item = parent.add(Menu.NONE, i, i, model.labels[i]);
            item.setEnabled((model.flags[i] & 1) != 0);
            if ((model.flags[i] & 2) != 0) {
                item.setCheckable(true);
                item.setChecked((model.flags[i] & 4) != 0);
            }
            String shortcut = model.shortcuts[i];
            if (shortcut != null && shortcut.length() == 1) {
                item.setAlphabeticShortcut(shortcut.charAt(0));
            }
        }
        return true;
    }

    static boolean choose(RnActivity activity, MenuItem item) {
        Model model = activity.menu;
        int index = item.getItemId();
        if (model == null || index < 0 || index >= model.ids.length) {
            return false;
        }
        RnBridge.nativeViewEvent(activity.window, MENU_TAG, Rn.EV_CLICK, 0, 0, model.ids[index]);
        return true;
    }

    /** Shows a context menu of {@code labels} anchored at {@code anchor}. */
    static void popup(final Activity activity, View anchor, final long window, final String[] ids,
        String[] labels, boolean[] enabled) {
        PopupMenu popup = new PopupMenu(activity, anchor);
        for (int i = 0; i < ids.length; i++) {
            popup.getMenu().add(Menu.NONE, i, i, labels[i]).setEnabled(enabled[i]);
        }
        popup.setOnMenuItemClickListener(new PopupMenu.OnMenuItemClickListener() {
            @Override
            public boolean onMenuItemClick(MenuItem item) {
                RnBridge.nativeViewEvent(window, MENU_TAG, Rn.EV_CLICK, 1, 0,
                    ids[item.getItemId()]);
                return true;
            }
        });
        popup.show();
    }
}
