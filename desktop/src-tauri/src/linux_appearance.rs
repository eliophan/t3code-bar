use gtk::prelude::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Manager;

type Values = BTreeMap<String, String>;

#[derive(Clone, Copy, Debug)]
struct Rgb([f64; 3]);

impl Rgb {
    const BLACK: Self = Self([0.0; 3]);
    const WHITE: Self = Self([1.0; 3]);

    fn parse(value: &str) -> Option<Self> {
        let hex = value.strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        let mut channels = [0.0; 3];
        for (i, channel) in channels.iter_mut().enumerate() {
            *channel = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()? as f64 / 255.0;
        }
        Some(Self(channels))
    }

    fn mix(self, other: Self, amount: f64) -> Self {
        Self(std::array::from_fn(|i| {
            ((self.0[i] * (1.0 - amount) + other.0[i] * amount) * 255.0).round() / 255.0
        }))
    }

    fn css(self) -> String {
        format!(
            "#{:02x}{:02x}{:02x}",
            (self.0[0] * 255.0).round() as u8,
            (self.0[1] * 255.0).round() as u8,
            (self.0[2] * 255.0).round() as u8
        )
    }

    fn luminance(self) -> f64 {
        let linear = self.0.map(|v| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        });
        linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
    }

    fn contrast(self, other: Self) -> f64 {
        let a = self.luminance();
        let b = other.luminance();
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn readable(self, surfaces: &[Self], minimum: f64) -> Self {
        let score = |c: Self| {
            surfaces
                .iter()
                .map(|bg| c.contrast(*bg))
                .fold(f64::INFINITY, f64::min)
        };
        if score(self) >= minimum {
            return self;
        }
        let target = if score(Self::WHITE) > score(Self::BLACK) {
            Self::WHITE
        } else {
            Self::BLACK
        };
        for step in 1..=100 {
            let candidate = self.mix(target, step as f64 / 100.0);
            // Check the serialized 8-bit color that WebKit actually receives.
            let quantized = Self::parse(&candidate.css()).unwrap();
            if score(quantized) >= minimum {
                return quantized;
            }
        }
        target
    }
}

fn opaque(value: &str, under: Rgb) -> Rgb {
    let Some(hex) = color(value) else {
        return under;
    };
    let Some(rgb) = Rgb::parse(&hex[..7]) else {
        return under;
    };
    let alpha = hex
        .get(7..9)
        .and_then(|v| u8::from_str_radix(v, 16).ok())
        .unwrap_or(255);
    under.mix(rgb, alpha as f64 / 255.0)
}

fn readable_colors(
    vars: &mut Values,
    popup: &str,
    text: &str,
    muted: &str,
    accent: &str,
    urgent: &str,
) {
    let bg = opaque(popup, Rgb([0.063, 0.075, 0.082]));
    let fg = opaque(text, bg).readable(&[bg], 7.0);
    let bg = bg.readable(&[fg], 7.0);
    let normal = bg.mix(fg, 0.04).readable(&[fg], 4.5);
    let hover = bg.mix(fg, 0.08).readable(&[fg], 4.5);
    let selected = bg.mix(fg, 0.14).readable(&[fg], 4.5);
    let pressed = bg.mix(fg, 0.18).readable(&[fg], 4.5);
    let surfaces = [bg, normal, hover, selected, pressed];
    let fg = fg.readable(&surfaces, 4.5);
    let secondary = opaque(muted, bg).readable(&surfaces, 4.5);
    for (name, rgb) in [
        ("bg", bg),
        ("fg", fg),
        ("secondary", secondary),
        ("muted", secondary),
        ("normal", normal),
        ("hover", hover),
        ("selected", selected),
        ("focus", hover),
        ("pressed", pressed),
        ("accent", opaque(accent, bg).readable(&surfaces, 4.5)),
        ("urgent", opaque(urgent, bg).readable(&surfaces, 4.5)),
        ("control-border", bg.mix(fg, 0.35).readable(&[bg], 3.0)),
        ("hover-border", bg.mix(fg, 0.55).readable(&[hover], 3.0)),
        ("focus-border", opaque(accent, bg).readable(&surfaces, 3.0)),
    ] {
        vars.insert(format!("--linux-{name}"), rgb.css());
    }
    vars.insert(
        "--linux-color-scheme".into(),
        if bg.luminance() > 0.179 {
            "light"
        } else {
            "dark"
        }
        .into(),
    );
}

struct NativeTheme {
    palette: toml::Value,
    font: String,
    font_size: f64,
}

fn native_theme(app: &tauri::AppHandle) -> NativeTheme {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let target = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = target
            .get_webview_window("main")
            .and_then(|w| w.gtk_window().ok())
        else {
            return;
        };
        let context = window.style_context();
        let mut palette = toml::map::Map::new();
        for (role, name) in [
            ("background", "theme_bg_color"),
            ("foreground", "theme_fg_color"),
            ("accent", "theme_selected_bg_color"),
            ("muted", "insensitive_fg_color"),
            ("red", "error_color"),
        ] {
            if let Some(c) = context.lookup_color(name) {
                palette.insert(
                    role.into(),
                    toml::Value::String(Rgb([c.red(), c.green(), c.blue()]).css()),
                );
            }
        }
        let font = gtk::Settings::default()
            .and_then(|s| s.gtk_font_name())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Sans 10".into());
        let (family, points) = font.rsplit_once(' ').unwrap_or((&font, "10"));
        let size = points.parse::<f64>().unwrap_or(10.0) * 96.0 / 72.0;
        let family = serde_json::to_string(family).unwrap_or_else(|_| "\"sans-serif\"".into());
        let _ = tx.send(NativeTheme {
            palette: toml::Value::Table(palette),
            font: format!("{family}, sans-serif"),
            font_size: size.clamp(12.0, 24.0),
        });
    });
    rx.recv_timeout(Duration::from_secs(2))
        .unwrap_or_else(|_| NativeTheme {
            palette: toml::Value::Table(Default::default()),
            font: "sans-serif".into(),
            font_size: 13.0,
        })
}

fn xdg_home(variable: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|p| p.join(fallback))
        })
}

fn read_toml(path: &Path) -> toml::Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| contents.parse().ok())
        .unwrap_or_else(|| toml::Value::Table(Default::default()))
}

fn parse_shell(contents: &str) -> toml::Value {
    let mut root = toml::map::Map::new();
    let mut section = String::new();
    for line in contents.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            if let Some((name, tail)) = header.split_once(']') {
                if tail.trim().is_empty() || tail.trim().starts_with('#') {
                    section = name.trim().to_owned();
                }
            }
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if section.is_empty()
            || key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            continue;
        }
        let raw = raw.trim();
        let value = if raw.starts_with('"') || raw.starts_with('\'') {
            let quote = raw.chars().next().unwrap();
            let Some(end) = raw[1..].find(quote) else {
                continue;
            };
            &raw[1..end + 1]
        } else {
            raw.split('#').next().unwrap_or_default().trim()
        };
        if value.is_empty() {
            continue;
        }
        let table = root
            .entry(section.clone())
            .or_insert_with(|| toml::Value::Table(Default::default()));
        table
            .as_table_mut()
            .unwrap()
            .insert(key.into(), toml::Value::String(value.into()));
    }
    toml::Value::Table(root)
}

fn read_shell(path: &Path) -> toml::Value {
    parse_shell(&std::fs::read_to_string(path).unwrap_or_default())
}

fn string<'a>(table: &'a toml::Value, path: &str) -> Option<&'a str> {
    path.split('.')
        .try_fold(table, |value, key| value.get(key))?
        .as_str()
}

fn number(table: &toml::Value, path: &str) -> Option<f64> {
    let value = path
        .split('.')
        .try_fold(table, |value, key| value.get(key))?;
    value
        .as_float()
        .or_else(|| value.as_integer().map(|n| n as f64))
        .or_else(|| value.as_str()?.parse().ok())
}

fn color(value: &str) -> Option<String> {
    let token = value.split_whitespace().next()?;
    let hex = token.strip_prefix('#').or_else(|| {
        token
            .strip_prefix("rgba(")
            .and_then(|s| s.strip_suffix(')'))
    })?;
    if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{hex}"))
}

fn resolve_color(value: &str, shell: &toml::Value, palette: &toml::Value) -> Option<String> {
    let mut token = value.split_whitespace().next()?;
    for _ in 0..8 {
        if let Some(parsed) = color(token) {
            return Some(parsed);
        }
        token = string(shell, token).or_else(|| string(palette, token))?;
        token = token.split_whitespace().next()?;
    }
    None
}

fn css_color(value: &str, alpha: f64) -> String {
    let hex = value.trim_start_matches('#');
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap();
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap();
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap();
    let existing_alpha = if hex.len() == 8 {
        u8::from_str_radix(&hex[6..8], 16).unwrap() as f64 / 255.0
    } else {
        1.0
    };
    format!(
        "rgba({r}, {g}, {b}, {:.3})",
        alpha.clamp(0.0, 1.0) * existing_alpha
    )
}

fn merge(base: &mut toml::Value, overlay: toml::Value) {
    if let (Some(base), toml::Value::Table(overlay)) = (base.as_table_mut(), overlay) {
        for (key, value) in overlay {
            if let Some(existing) = base.get_mut(&key) {
                if existing.is_table() && value.is_table() {
                    merge(existing, value);
                    continue;
                }
            }
            base.insert(key, value);
        }
    }
}

fn appearance(palette: &toml::Value, shell: &toml::Value, rounding: u32) -> Values {
    let mut vars = Values::new();
    vars.insert(
        "--linux-color-scheme".into(),
        if string(palette, "mode") == Some("light") {
            "light"
        } else {
            "dark"
        }
        .into(),
    );
    let get = |key: &str, fallback: &str| {
        string(palette, key)
            .and_then(color)
            .unwrap_or_else(|| fallback.into())
    };
    let background = get("background", &get("color0", "#101315"));
    let foreground = get("foreground", &get("color7", "#cacccc"));
    let accent = get("accent", &get("color4", "#cacccc"));
    let muted = get("muted", &get("color8", &foreground));
    let urgent = get("red", &get("color1", "#a55555"));
    let roles = [
        ("background", &background),
        ("foreground", &foreground),
        ("accent", &accent),
        ("muted", &muted),
        ("urgent", &urgent),
    ];
    let resolve = |key: &str, fallback: &str| {
        string(shell, key)
            .and_then(|v| {
                roles
                    .iter()
                    .find(|(role, _)| *role == v)
                    .map(|(_, value)| (*value).clone())
                    .or_else(|| resolve_color(v, shell, palette))
            })
            .unwrap_or_else(|| fallback.into())
    };
    let popup = resolve("popups.background", &background);
    let text = resolve("popups.text", &foreground);
    let border = resolve("popups.border", &accent);
    let alpha = |key: &str, default: f64| {
        number(shell, key)
            .filter(|n| n.is_finite())
            .unwrap_or(default)
            .clamp(0.0, 1.0)
    };
    let entries = [
        (
            "border",
            css_color(&border, alpha("popups.border-alpha", 1.0)),
        ),
        ("radius", format!("{}px", rounding.min(100))),
        (
            "switch-radius",
            if rounding == 0 { "0px" } else { "999px" }.into(),
        ),
        (
            "font-size",
            format!(
                "{}px",
                number(shell, "font.base-size")
                    .unwrap_or(12.0)
                    .clamp(8.0, 36.0)
                    .round()
            ),
        ),
    ];
    for (key, value) in entries {
        vars.insert(format!("--linux-{key}"), value);
    }

    let font_size = number(shell, "font.base-size")
        .unwrap_or(12.0)
        .clamp(8.0, 36.0);
    let scale_with_font = string(shell, "spacing.scale-with-font") != Some("false")
        && shell
            .get("spacing")
            .and_then(|v| v.get("scale-with-font"))
            .and_then(toml::Value::as_bool)
            != Some(false);
    let spacing_scale = number(shell, "spacing.scale")
        .unwrap_or(1.0)
        .clamp(0.25, 4.0)
        * if scale_with_font {
            font_size / 12.0
        } else {
            1.0
        };
    for (key, fallback) in [
        ("control-padding-x", 10.0),
        ("control-padding-y", 6.0),
        ("input-padding-y", 7.0),
        ("control-height", 28.0),
        ("panel-padding", 18.0),
        ("row-gap", 8.0),
    ] {
        let value = number(shell, &format!("spacing.{key}"))
            .unwrap_or(fallback * spacing_scale)
            .max(0.0);
        vars.insert(format!("--linux-{key}"), format!("{value}px"));
    }
    vars.insert("--linux-spacing-scale".into(), spacing_scale.to_string());
    readable_colors(&mut vars, &popup, &text, &muted, &accent, &urgent);
    vars
}

fn rounding() -> u32 {
    std::process::Command::new("hyprctl")
        .args(["-j", "getoption", "decoration:rounding"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| v.get("int")?.as_u64())
        .unwrap_or(0)
        .min(100) as u32
}

fn font_family() -> String {
    let name = std::process::Command::new("fc-match")
        .args(["-f", "%{family[0]}", "monospace"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let name = name.trim();
    if !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
    {
        format!("\"{name}\", monospace")
    } else {
        "monospace".into()
    }
}

fn current(rounding: u32, font: &str, native: NativeTheme) -> Values {
    let theme = xdg_home("XDG_STATE_HOME", ".local/state").map(|p| p.join("omarchy/current/theme"));
    let omarchy_palette = theme
        .as_ref()
        .map(|p| read_toml(&p.join("colors.toml")))
        .unwrap_or_else(|| read_toml(Path::new("")));
    let is_omarchy = string(&omarchy_palette, "background")
        .and_then(color)
        .is_some();
    let mut palette = native.palette;
    if is_omarchy {
        merge(&mut palette, omarchy_palette);
    }
    let mut shell = theme
        .as_ref()
        .map(|p| read_shell(&p.join("shell.toml")))
        .unwrap_or_else(|| read_toml(Path::new("")));
    if let Some(config) = xdg_home("XDG_CONFIG_HOME", ".config") {
        if is_omarchy {
            merge(&mut shell, read_shell(&config.join("omarchy/shell.toml")));
        }
    }
    if !is_omarchy {
        shell = toml::Value::Table(Default::default());
        let mut font = toml::map::Map::new();
        font.insert("base-size".into(), toml::Value::Float(native.font_size));
        shell
            .as_table_mut()
            .unwrap()
            .insert("font".into(), toml::Value::Table(font));
    }
    let mut vars = appearance(&palette, &shell, rounding);
    vars.insert(
        "--linux-font-family".into(),
        if is_omarchy { font.into() } else { native.font },
    );
    vars
}

fn apply(app: tauri::AppHandle, vars: Values) {
    let Ok(json) = serde_json::to_string(&vars) else {
        return;
    };
    let script = format!("document.documentElement.classList.add('linux-desktop'); Object.entries({json}).forEach(([k,v]) => document.documentElement.style.setProperty(k,v));");
    let target = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = target.get_webview_window("main") {
            let _ = window.eval(&script);
        }
    });
}

pub fn refresh(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let native = native_theme(&app);
        apply(app, current(rounding(), &font_family(), native));
    });
}

pub fn watch(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut previous = Values::new();
        let mut radius = rounding();
        let mut font = font_family();
        let mut tick = 0;
        loop {
            let vars = current(radius, &font, native_theme(&app));
            if vars != previous {
                apply(app.clone(), vars.clone());
                previous = vars;
            }
            std::thread::sleep(Duration::from_secs(2));
            tick += 1;
            if tick == 5 {
                radius = rounding();
                font = font_family();
                tick = 0;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_text_in_dark_light_and_low_contrast_themes() {
        for (background, foreground, muted, accent) in [
            ("#0c0b0c", "#fafcfb", "#584e51", "#b59790"),
            ("#fafafa", "#222222", "#dddddd", "#aaccff"),
            ("#888888", "#888888", "#888888", "#888888"),
            ("#5b7398", "#49b2fb", "#952d99", "#49b2fb"),
            ("#10203080", "#eeeeee80", "#102030", "#123456"),
        ] {
            let palette = format!("background = '{background}'\nforeground = '{foreground}'\nmuted = '{muted}'\naccent = '{accent}'").parse().unwrap();
            let vars = appearance(&palette, &parse_shell(""), 0);
            for role in ["fg", "secondary", "accent", "urgent"] {
                let fg = Rgb::parse(&vars[&format!("--linux-{role}")]).unwrap();
                for surface in ["bg", "normal", "hover", "selected", "focus", "pressed"] {
                    let bg = Rgb::parse(&vars[&format!("--linux-{surface}")]).unwrap();
                    assert!(
                        fg.contrast(bg) >= 4.5,
                        "{background} {role} on {surface}: {}",
                        fg.contrast(bg)
                    );
                }
            }
        }
    }
    #[test]
    fn accepts_omarchy_bare_roles_width_lists_and_scaling() {
        let palette = "background = '#101010'\nforeground = '#ffffff'\naccent = '#aabbcc'"
            .parse()
            .unwrap();
        let shell = parse_shell("[popups]\nbackground = background # comment\ntext = foreground\n[controls]\nnormal-color = accent\nnormal-border-width = 1 2\n[font]\nbase-size = 18\n[spacing]\nscale = 1.25\ncontrol-padding-x = 17");
        let vars = appearance(&palette, &shell, 0);
        assert_eq!(vars["--linux-bg"], "#101010");
        assert_eq!(vars["--linux-font-size"], "18px");
        assert_eq!(vars["--linux-control-padding-x"], "17px");
        assert_eq!(vars["--linux-control-height"], "52.5px");
    }
    #[test]
    fn resolves_shell_roles_and_rejects_injection() {
        let palette = "background = '#123456'\nforeground = '#eeeeee'\naccent = '#abcdef'"
            .parse()
            .unwrap();
        let shell = "[hyprland]\nactive-border = 'rgba(123456ee) rgba(abcdefee)'\n[popups]\nborder = 'hyprland.active-border'\nbackground = '#000000; color: red'\n[controls]\nnormal-fill-alpha = 0.2".parse().unwrap();
        let vars = appearance(&palette, &shell, 0);
        assert_eq!(vars["--linux-border"], "rgba(18, 52, 86, 0.933)");
        assert_eq!(vars["--linux-bg"], "#123456");
        assert_eq!(vars["--linux-radius"], "0px");
    }
    #[test]
    fn merges_user_overrides_and_handles_cycles() {
        let palette = "accent = '#aabbcc'".parse().unwrap();
        let mut shell = "[popups]\nborder = 'popups.border'\ntext = '#112233'"
            .parse()
            .unwrap();
        merge(&mut shell, "[popups]\ntext = '#334455'".parse().unwrap());
        let vars = appearance(&palette, &shell, 4);
        assert_eq!(vars["--linux-border"], "rgba(170, 187, 204, 1.000)");
        assert!(
            Rgb::parse(&vars["--linux-fg"])
                .unwrap()
                .contrast(Rgb::parse(&vars["--linux-bg"]).unwrap())
                >= 4.5
        );
        assert_eq!(vars["--linux-radius"], "4px");
    }
}
