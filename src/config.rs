use serde::Deserialize;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Config {
    pub theme: Option<String>,
    pub width: Option<u16>,
    pub spacing: Option<String>,
    pub toc: Option<bool>,
    pub frontmatter: Option<bool>,
    pub behavior: Option<BehaviorConfig>,
    pub keybindings: Option<KeybindingsConfig>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct BehaviorConfig {
    /// When true, closing a file via q/Esc returns to the browser instead of exiting.
    pub browser_loop: Option<bool>,
    /// When false, ink does not grab the mouse, so the terminal's own
    /// click-to-open (OSC 8) and text selection keep working. Default true
    /// (mouse wheel scrolls the document).
    pub mouse_capture: Option<bool>,
    /// How copies reach the clipboard: "auto" | "osc52" | "native" | "off".
    pub clipboard: Option<String>,
    /// Draw borders, bullets and markers with 7-bit ASCII. Unset: automatic
    /// (on for the Linux console and non-UTF-8 locales).
    pub ascii: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct KeybindingsConfig {
    /// Built-in preset name: "default" | "vim" | "emacs".
    pub preset: Option<String>,
    /// Per-action overrides applied on top of the preset.
    /// Key is the action ID (e.g. "scroll_down"), value is the list of key strings.
    pub bindings: Option<HashMap<String, Vec<String>>>,
}

/// The ink config directory: `$XDG_CONFIG_HOME/ink` when that variable is
/// set to an absolute path (on every platform), otherwise `<platform>/ink`
/// (`~/.config` on Linux, `~/Library/Application Support` on macOS,
/// `%APPDATA%` on Windows).
///
/// One exception keeps upgrades safe: if `$XDG_CONFIG_HOME/ink` does not
/// exist yet but `<platform>/ink` does, the existing directory stays in use,
/// so a config written before ink honored the variable is not orphaned.
pub fn config_dir() -> Option<PathBuf> {
    resolve_config_dir(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        dirs::config_dir(),
        |p| p.is_dir(),
    )
}

/// Pure form of [`config_dir`], so the rule is testable without touching the
/// process environment or the disk. Per the XDG Base Directory spec, an empty
/// or relative `$XDG_CONFIG_HOME` is ignored.
fn resolve_config_dir(
    xdg: Option<&OsStr>,
    platform: Option<PathBuf>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let xdg = xdg
        .map(Path::new)
        .filter(|p| p.is_absolute())
        .map(|p| p.join("ink"));
    let platform = platform.map(|p| p.join("ink"));
    match (xdg, platform) {
        (Some(x), Some(p)) if !exists(&x) && exists(&p) => Some(p),
        (Some(x), _) => Some(x),
        (None, p) => p,
    }
}

/// Directory holding user theme files (`<config dir>/themes`).
pub fn themes_dir() -> Option<PathBuf> {
    Some(config_dir()?.join("themes"))
}

/// Resolved path of the active config file, if a config dir can be found.
pub fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join("config.toml"))
}

/// Human-readable string for `ink config path`.
pub fn config_path_display() -> String {
    config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<no config dir on this platform>".to_string())
}

/// Load config from [`config_path`], discarding any warnings. Prefer
/// [`load_config_with_warnings`] where the warnings can reach the user.
pub fn load_config() -> Option<Config> {
    load_config_with_warnings().config
}

/// Valid `width` range in columns, shared by the config file and `--width`.
pub const WIDTH_RANGE: std::ops::RangeInclusive<u16> = 20..=1000;

/// Valid `spacing` values, shared by the config file and `--spacing`.
const SPACING_VALUES: [&str; 3] = ["compact", "normal", "relaxed"];

/// Outcome of reading the config file: the parsed config (if any) plus
/// human-readable warnings for the CLI to print before the TUI starts.
#[derive(Debug, Default)]
pub struct ConfigLoad {
    pub config: Option<Config>,
    pub warnings: Vec<String>,
}

/// Load config from [`config_path`]. A missing file is silent; an unreadable
/// file or a TOML syntax error yields one warning and no config; a bad value
/// or an unknown key yields a warning naming that key while every other key
/// still applies.
pub fn load_config_with_warnings() -> ConfigLoad {
    let Some(path) = config_path() else {
        return ConfigLoad::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => parse_config(&content, &path.display().to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ConfigLoad::default(),
        Err(e) => ConfigLoad {
            config: None,
            warnings: vec![format!(
                "ink: cannot read config {}: {e} (using defaults)",
                path.display()
            )],
        },
    }
}

/// Parse config text leniently. `path` only labels the warnings.
pub fn parse_config(content: &str, path: &str) -> ConfigLoad {
    let mut warnings = Vec::new();
    let mut table = match content.parse::<toml::Table>() {
        Ok(t) => t,
        Err(e) => {
            let at = e
                .span()
                .map(|s| {
                    let before = &content[..s.start.min(content.len())];
                    let line = before.matches('\n').count() + 1;
                    let col = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
                    format!("line {line}, column {col}: ")
                })
                .unwrap_or_default();
            warnings.push(format!(
                "ink: config error in {path}: {at}{} (using defaults)",
                e.message().trim()
            ));
            return ConfigLoad {
                config: None,
                warnings,
            };
        }
    };
    let mut w = Warn {
        path,
        warnings: &mut warnings,
    };

    let mut width: Option<u16> = w.take(&mut table, "", "width");
    if let Some(n) = width.filter(|n| !WIDTH_RANGE.contains(n)) {
        w.push(format!(
            "`width` = {n} is out of range ({}..={}); ignoring it",
            WIDTH_RANGE.start(),
            WIDTH_RANGE.end()
        ));
        width = None;
    }
    let mut spacing: Option<String> = w.take(&mut table, "", "spacing");
    if let Some(s) = spacing.as_deref().filter(|s| !SPACING_VALUES.contains(s)) {
        w.push(format!(
            "`spacing` = \"{s}\" is not one of: {}; ignoring it",
            SPACING_VALUES.join(", ")
        ));
        spacing = None;
    }
    let mut config = Config {
        theme: w.take(&mut table, "", "theme"),
        width,
        spacing,
        toc: w.take(&mut table, "", "toc"),
        frontmatter: w.take(&mut table, "", "frontmatter"),
        behavior: None,
        keybindings: None,
    };
    if let Some(mut t) = w.take::<toml::Table>(&mut table, "", "behavior") {
        config.behavior = Some(BehaviorConfig {
            browser_loop: w.take(&mut t, "behavior.", "browser_loop"),
            mouse_capture: w.take(&mut t, "behavior.", "mouse_capture"),
            clipboard: w.take(&mut t, "behavior.", "clipboard"),
            ascii: w.take(&mut t, "behavior.", "ascii"),
        });
        w.unknown(&t, "behavior.");
    }
    if let Some(mut t) = w.take::<toml::Table>(&mut table, "", "keybindings") {
        config.keybindings = Some(KeybindingsConfig {
            preset: w.take(&mut t, "keybindings.", "preset"),
            bindings: w.take(&mut t, "keybindings.", "bindings"),
        });
        w.unknown(&t, "keybindings.");
    }
    w.unknown(&table, "");

    ConfigLoad {
        config: Some(config),
        warnings,
    }
}

/// Warning collector for [`parse_config`].
struct Warn<'a> {
    path: &'a str,
    warnings: &'a mut Vec<String>,
}

impl Warn<'_> {
    fn push(&mut self, msg: String) {
        self.warnings
            .push(format!("ink: config error in {}: {msg}", self.path));
    }

    /// Remove `key` from `table` and deserialize it; a value of the wrong
    /// type warns and is dropped, leaving the rest of the config intact.
    fn take<T: serde::de::DeserializeOwned>(
        &mut self,
        table: &mut toml::Table,
        section: &str,
        key: &str,
    ) -> Option<T> {
        let value = table.remove(key)?;
        match value.try_into::<T>() {
            Ok(v) => Some(v),
            Err(e) => {
                let msg = e.message().trim().to_string();
                self.push(format!("`{section}{key}`: {msg}; ignoring it"));
                None
            }
        }
    }

    /// Warn about every key left in `table` after the known ones were taken.
    fn unknown(&mut self, table: &toml::Table, section: &str) {
        for key in table.keys() {
            self.warnings.push(format!(
                "ink: unknown config key `{section}{key}` in {} (ignored)",
                self.path
            ));
        }
    }
}

/// Persist the chosen theme to the config file, preserving existing content
/// and comments. Creates the file (and parent dir) if absent.
pub fn set_theme(name: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    let path = config_path().context("no config directory on this platform")?;
    let mut doc = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| c.parse::<toml_edit::DocumentMut>().ok())
        .unwrap_or_default();
    doc["theme"] = toml_edit::value(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, doc.to_string())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform() -> Option<PathBuf> {
        Some(PathBuf::from("platform-config"))
    }

    /// An absolute path on every OS (`/xdg` is not absolute on Windows).
    fn abs_xdg() -> PathBuf {
        std::env::temp_dir().join("xdg")
    }

    const NOTHING: fn(&Path) -> bool = |_| false;

    #[test]
    fn syntax_error_warns_once_with_position() {
        let load = parse_config("theme = \"nord\"\nwidth = \n", "cfg.toml");
        assert!(load.config.is_none());
        assert_eq!(load.warnings.len(), 1, "{:?}", load.warnings);
        let w = &load.warnings[0];
        assert!(
            w.starts_with("ink: config error in cfg.toml: line 2"),
            "{w}"
        );
        assert!(w.contains("using defaults"), "{w}");
    }

    #[test]
    fn bad_value_warns_and_keeps_the_rest() {
        let src = "width = \"wide\"\ntheme = \"nord\"\n[keybindings]\npreset = \"vim\"\n";
        let load = parse_config(src, "cfg.toml");
        let cfg = load.config.expect("rest of the config still applies");
        assert_eq!(cfg.width, None);
        assert_eq!(cfg.theme.as_deref(), Some("nord"));
        assert_eq!(cfg.keybindings.unwrap().preset.as_deref(), Some("vim"));
        assert_eq!(load.warnings.len(), 1, "{:?}", load.warnings);
        assert!(load.warnings[0].contains("`width`"), "{:?}", load.warnings);
    }

    #[test]
    fn unknown_keys_warn_by_name() {
        let src = "widht = 90\nwidth = 90\n[behavior]\nmouse_capure = false\n";
        let load = parse_config(src, "cfg.toml");
        assert_eq!(load.config.unwrap().width, Some(90));
        let all = load.warnings.join("\n");
        assert!(
            all.contains("unknown config key `widht` in cfg.toml"),
            "{all}"
        );
        assert!(all.contains("`behavior.mouse_capure`"), "{all}");
        assert_eq!(load.warnings.len(), 2);
    }

    #[test]
    fn out_of_range_values_warn_and_fall_back() {
        let load = parse_config("width = 5\nspacing = \"bogus\"\ntheme = 5\n", "c");
        let cfg = load.config.unwrap();
        assert_eq!((cfg.width, cfg.spacing, cfg.theme), (None, None, None));
        assert_eq!(load.warnings.len(), 3, "{:?}", load.warnings);
    }

    #[test]
    fn valid_config_is_quiet() {
        let load = parse_config(crate::cli::STARTER_CONFIG, "c");
        assert!(load.warnings.is_empty(), "{:?}", load.warnings);
        let load = parse_config(
            "width = 90\nspacing = \"relaxed\"\n[behavior]\nclipboard = \"off\"\n\
             [keybindings.bindings]\ntoggle_toc = [\"ctrl-t\"]\n",
            "c",
        );
        assert!(load.warnings.is_empty(), "{:?}", load.warnings);
        assert_eq!(load.config.unwrap().width, Some(90));
    }

    #[test]
    fn xdg_set_is_used() {
        let xdg = abs_xdg();
        assert_eq!(
            resolve_config_dir(Some(xdg.as_os_str()), platform(), NOTHING),
            Some(xdg.join("ink"))
        );
    }

    #[test]
    fn xdg_wins_when_both_dirs_exist() {
        let xdg = abs_xdg();
        assert_eq!(
            resolve_config_dir(Some(xdg.as_os_str()), platform(), |_| true),
            Some(xdg.join("ink"))
        );
    }

    #[test]
    fn existing_platform_dir_is_kept_until_xdg_dir_exists() {
        let xdg = abs_xdg();
        let legacy = PathBuf::from("platform-config").join("ink");
        assert_eq!(
            resolve_config_dir(Some(xdg.as_os_str()), platform(), |p| p == legacy),
            Some(legacy.clone())
        );
    }

    #[test]
    fn xdg_unset_falls_back_to_platform() {
        assert_eq!(
            resolve_config_dir(None, platform(), NOTHING),
            Some(PathBuf::from("platform-config").join("ink"))
        );
    }

    #[test]
    fn xdg_empty_falls_back_to_platform() {
        assert_eq!(
            resolve_config_dir(Some(OsStr::new("")), platform(), NOTHING),
            Some(PathBuf::from("platform-config").join("ink"))
        );
    }

    #[test]
    fn xdg_relative_falls_back_to_platform() {
        assert_eq!(
            resolve_config_dir(Some(OsStr::new("relative/dir")), platform(), NOTHING),
            Some(PathBuf::from("platform-config").join("ink"))
        );
    }

    #[test]
    fn xdg_used_without_platform_dir() {
        let xdg = abs_xdg();
        assert_eq!(
            resolve_config_dir(Some(xdg.as_os_str()), None, NOTHING),
            Some(xdg.join("ink"))
        );
        assert_eq!(resolve_config_dir(None, None, NOTHING), None);
    }
}
