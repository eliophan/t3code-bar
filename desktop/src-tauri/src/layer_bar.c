#include <gdk/gdkwayland.h>
#include <gtk-layer-shell.h>

int t3_layer_bar_init(GtkWindow *window) {
    if (!GDK_IS_WAYLAND_DISPLAY(gtk_widget_get_display(GTK_WIDGET(window))))
        return 0;
    if (gtk_widget_get_realized(GTK_WIDGET(window)))
        return 1;
    if (!gtk_layer_is_supported() || gtk_layer_get_protocol_version() < 4)
        return 2;

    gtk_layer_init_for_window(window);
    gtk_layer_set_namespace(window, "t3-code-bar");
    gtk_layer_set_layer(window, GTK_LAYER_SHELL_LAYER_OVERLAY);
    gtk_layer_set_anchor(window, GTK_LAYER_SHELL_EDGE_TOP, TRUE);
    gtk_layer_set_anchor(window, GTK_LAYER_SHELL_EDGE_RIGHT, TRUE);
    gtk_layer_set_margin(window, GTK_LAYER_SHELL_EDGE_TOP, 5);
    gtk_layer_set_margin(window, GTK_LAYER_SHELL_EDGE_RIGHT, 5);
    gtk_layer_set_exclusive_zone(window, 0);
    gtk_layer_set_keyboard_mode(window, GTK_LAYER_SHELL_KEYBOARD_MODE_ON_DEMAND);
    return gtk_layer_is_layer_window(window) ? 0 : 3;
}
