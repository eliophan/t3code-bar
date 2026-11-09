#include <gdk/gdkwayland.h>
#include <stdlib.h>
#include <string.h>
#include <wayland-client.h>
#include "hyprland-focus-grab-v1-client-protocol.h"

struct focus_grab {
    struct wl_registry *registry;
    struct hyprland_focus_grab_manager_v1 *manager;
    struct hyprland_focus_grab_v1 *grab;
    struct wl_surface *surface;
    void (*cleared)(void *);
    void *data;
};

static void grab_cleared(void *data, struct hyprland_focus_grab_v1 *grab) {
    struct focus_grab *state = data;
    (void)grab;
    state->cleared(state->data);
}

static const struct hyprland_focus_grab_v1_listener grab_listener = {
    .cleared = grab_cleared,
};

static void global(void *data, struct wl_registry *registry, uint32_t name,
                   const char *interface, uint32_t version) {
    struct focus_grab *state = data;
    (void)version;
    if (state->manager || strcmp(interface, "hyprland_focus_grab_manager_v1") != 0)
        return;
    state->manager = wl_registry_bind(registry, name, &hyprland_focus_grab_manager_v1_interface, 1);
    if (!state->manager)
        return;
    state->grab = hyprland_focus_grab_manager_v1_create_grab(state->manager);
    if (!state->grab)
        return;
    hyprland_focus_grab_v1_add_listener(state->grab, &grab_listener, state);
    hyprland_focus_grab_v1_add_surface(state->grab, state->surface);
    hyprland_focus_grab_v1_commit(state->grab);
}

static void global_remove(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data;
    (void)registry;
    (void)name;
}

static const struct wl_registry_listener registry_listener = {
    .global = global,
    .global_remove = global_remove,
};

struct focus_grab *t3_focus_grab_start(GdkWindow *window, void (*cleared)(void *), void *data) {
    if (!window || !GDK_IS_WAYLAND_WINDOW(window))
        return NULL;
    GdkDisplay *display = gdk_window_get_display(window);
    if (!GDK_IS_WAYLAND_DISPLAY(display))
        return NULL;
    struct wl_surface *surface = gdk_wayland_window_get_wl_surface(window);
    if (!surface)
        return NULL;
    struct focus_grab *state = calloc(1, sizeof(*state));
    if (!state)
        return NULL;
    state->surface = surface;
    state->cleared = cleared;
    state->data = data;
    state->registry = wl_display_get_registry(gdk_wayland_display_get_wl_display(display));
    if (!state->registry) {
        free(state);
        return NULL;
    }
    wl_registry_add_listener(state->registry, &registry_listener, state);
    return state;
}

void t3_focus_grab_stop(struct focus_grab *state) {
    if (state->grab)
        hyprland_focus_grab_v1_destroy(state->grab);
    if (state->manager)
        hyprland_focus_grab_manager_v1_destroy(state->manager);
    wl_registry_destroy(state->registry);
    free(state);
}
