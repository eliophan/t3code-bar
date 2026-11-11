fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        let gdk = pkg_config::probe_library("gdk-wayland-3.0").expect("gdk-wayland-3.0");
        pkg_config::probe_library("wayland-client").expect("wayland-client");
        let layer = pkg_config::Config::new()
            .atleast_version("0.6")
            .probe("gtk-layer-shell-0")
            .expect("gtk-layer-shell-0 >= 0.6");
        cc::Build::new()
            .includes(gdk.include_paths)
            .includes(layer.include_paths)
            .file("src/focus_grab/focus_grab.c")
            .file("src/focus_grab/hyprland-focus-grab-v1-protocol.c")
            .file("src/layer_bar.c")
            .compile("focus_grab");
        println!("cargo:rerun-if-changed=src/focus_grab");
        println!("cargo:rerun-if-changed=src/layer_bar.c");
    }
    tauri_build::build();
}
