//! Herdr does not hand its theme to plugins, so the popup works it out the way Herdr does: the
//! built-in theme named in config.toml's `[theme]` table, with the colors of `[theme.custom]` on
//! top. `[theme.custom.light]`, `[theme.custom.dark]` and `auto_switch` are not applied. A theme
//! this build does not know leaves the terminal's own colors.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use crate::themes::{BUILTIN, TOKENS};

/// The colors of Herdr's delete overlay. `None` leaves the terminal's color.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Palette {
    pub panel_bg: Option<Rgb>,
    pub surface0: Option<Rgb>,
    pub text: Option<Rgb>,
    pub subtext0: Option<Rgb>,
    pub red: Option<Rgb>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

fn config_path() -> PathBuf {
    if let Some(path) = env::var_os("HERDR_CONFIG_PATH").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("herdr").join("config.toml")
}

pub fn load() -> Palette {
    let config = fs::read_to_string(config_path()).unwrap_or_default();
    let (name, custom) = read_theme(&config);
    resolve(&name, &custom)
}

/// Returns a config's theme name and its `[theme.custom]` colors. It reads the simple
/// `key = "value"` lines those tables hold and skips the rest.
fn read_theme(config: &str) -> (String, HashMap<String, String>) {
    let mut name = String::new();
    let mut custom = HashMap::new();
    let mut table = String::new();
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            let header = line.split('#').next().unwrap_or_default();
            table = header.trim_matches(|c: char| c == '[' || c == ']' || c.is_whitespace()).to_string();
            continue;
        }
        if line.starts_with('#') || (table != "theme" && table != "theme.custom") {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(value) = quoted(value.trim()) else {
            continue;
        };
        if table == "theme.custom" {
            custom.insert(key.trim().to_string(), value.to_string());
        } else if key.trim() == "name" {
            name = value.to_string();
        }
    }
    (name, custom)
}

fn quoted(value: &str) -> Option<&str> {
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &value[1..];
    rest.find(quote).map(|end| &rest[..end])
}

/// The other names Herdr accepts for a built-in theme.
fn canonical(name: &str) -> String {
    let name = name.trim().to_lowercase().replace([' ', '_'], "-");
    match name.as_str() {
        "" => "catppuccin",
        "catppuccin-mocha" => "catppuccin",
        "latte" | "light" => "catppuccin-latte",
        "tokyonight" => "tokyo-night",
        "tokyo-day" | "tokyonight-day" => "tokyo-night-day",
        "gruvbox-dark" => "gruvbox",
        "onedark" => "one-dark",
        "onelight" => "one-light",
        "solarized-dark" => "solarized",
        "lotus" => "kanagawa-lotus",
        "rosepine" => "rose-pine",
        "rosepine-dawn" | "dawn" => "rose-pine-dawn",
        other => other,
    }
    .to_string()
}

fn resolve(name: &str, custom: &HashMap<String, String>) -> Palette {
    let name = canonical(name);
    let builtin = BUILTIN.iter().find(|(theme, _)| *theme == name).map(|(_, colors)| colors);
    let color = |token: &str| {
        let index = TOKENS.iter().position(|known| *known == token)?;
        custom
            .get(token)
            .and_then(|text| parse_color(text))
            .or_else(|| builtin.and_then(|colors| parse_color(colors[index])))
    };
    Palette {
        panel_bg: color("panel_bg"),
        surface0: color("surface0"),
        text: color("text"),
        subtext0: color("subtext0"),
        red: color("red"),
    }
}

/// Parses `#rrggbb`, `#rgb` and `rgb(r, g, b)`. Anything else leaves the terminal's color.
fn parse_color(text: &str) -> Option<Rgb> {
    let text = text.trim().to_lowercase();
    if let Some(hex) = text.strip_prefix('#') {
        let value = u32::from_str_radix(hex, 16).ok()?;
        return match hex.len() {
            6 => Some(Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)),
            3 => Some(Rgb(
                ((value >> 8) & 0xf) as u8 * 17,
                ((value >> 4) & 0xf) as u8 * 17,
                (value & 0xf) as u8 * 17,
            )),
            _ => None,
        };
    }
    let inner = text.strip_prefix("rgb(")?.strip_suffix(')')?;
    let mut parts = inner.split(',').map(|part| part.trim().parse::<u8>());
    let rgb = Rgb(parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?);
    parts.next().is_none().then_some(rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_builtin_theme_is_used() {
        let (name, custom) = read_theme("[theme]\nname = \"tokyo-night\"\nauto_switch = false\n");
        let palette = resolve(&name, &custom);
        assert_eq!(palette.panel_bg, Some(Rgb(0x1a, 0x1b, 0x26)));
        assert_eq!(palette.red, Some(Rgb(0xf7, 0x76, 0x8e)));
    }

    #[test]
    fn no_theme_means_catppuccin() {
        let (name, custom) = read_theme("[ui]\nname = \"nord\"\n");
        assert_eq!(resolve(&name, &custom).panel_bg, Some(Rgb(0x18, 0x18, 0x25)));
    }

    #[test]
    fn alias_resolves() {
        assert_eq!(
            resolve("Tokyo_Night", &HashMap::new()),
            resolve("tokyonight", &HashMap::new())
        );
    }

    #[test]
    fn custom_colors_go_on_top() {
        let config = "[theme]\nname = 'nord'\n[theme.custom] # mine\npanel_bg = \"#000\"\ntext = \"rgb(1, 2, 3)\"\n";
        let (name, custom) = read_theme(config);
        let palette = resolve(&name, &custom);
        assert_eq!(palette.panel_bg, Some(Rgb(0, 0, 0)));
        assert_eq!(palette.text, Some(Rgb(1, 2, 3)));
        assert_eq!(palette.red, Some(Rgb(0xbf, 0x61, 0x6a)));
    }

    #[test]
    fn unknown_theme_leaves_terminal_colors() {
        assert_eq!(resolve("terminal", &HashMap::new()), Palette::default());
        assert_eq!(resolve("made-up", &HashMap::new()), Palette::default());
    }

    #[test]
    fn bad_colors_are_ignored() {
        assert_eq!(parse_color("#12"), None);
        assert_eq!(parse_color("#zzzzzz"), None);
        assert_eq!(parse_color("rgb(1,2)"), None);
        assert_eq!(parse_color("rgb(1,2,3,4)"), None);
        assert_eq!(parse_color("rgb(300,0,0)"), None);
        assert_eq!(parse_color("reset"), None);
    }
}
