//! Facts about the machine and session that more than one module needs:
//! WSL, SSH, multiplexers, and whether the locale is UTF-8.
//!
//! Each check has a pure form taking the inputs explicitly, so the rules are
//! unit-testable without touching the process environment.

/// Non-empty value of an environment variable.
pub fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Running under the Windows Subsystem for Linux?
pub fn is_wsl() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    is_wsl_from(env_nonempty("WSL_DISTRO_NAME").is_some(), &release)
}

/// Pure form of [`is_wsl`]: WSL exports `WSL_DISTRO_NAME`, and its kernels
/// carry `microsoft` in the release string (`…-microsoft-standard-WSL2`).
pub fn is_wsl_from(distro_name_set: bool, kernel_release: &str) -> bool {
    distro_name_set || kernel_release.to_ascii_lowercase().contains("microsoft")
}

/// Inside an SSH session?
pub fn is_ssh() -> bool {
    env_nonempty("SSH_CONNECTION").is_some() || env_nonempty("SSH_TTY").is_some()
}

/// The locale variable that decides the character set, in POSIX precedence
/// (`LC_ALL` > `LC_CTYPE` > `LANG`), as (name, value).
pub fn effective_locale() -> Option<(&'static str, String)> {
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .find_map(|k| env_nonempty(k).map(|v| (k, v)))
}

/// Is the locale's character set UTF-8? `None` when no locale variable is set
/// (then the C locale applies, which is not UTF-8 — but plenty of macOS and
/// container setups leave all three unset while the terminal is UTF-8, so
/// callers treat `None` as "unknown", not as "ASCII").
pub fn locale_is_utf8() -> Option<bool> {
    effective_locale().map(|(_, v)| locale_value_is_utf8(&v))
}

/// Pure check on one locale value (`en_US.UTF-8`, `C.utf8`, `de_DE@euro`).
pub fn locale_value_is_utf8(value: &str) -> bool {
    let v = value.to_ascii_lowercase();
    v.contains("utf-8") || v.contains("utf8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_from_either_signal() {
        assert!(is_wsl_from(true, ""));
        assert!(is_wsl_from(false, "5.15.153.1-microsoft-standard-WSL2"));
        assert!(is_wsl_from(false, "4.4.0-19041-Microsoft"));
        assert!(!is_wsl_from(false, "6.8.0-45-generic"));
    }

    #[test]
    fn utf8_locale_spellings() {
        assert!(locale_value_is_utf8("en_US.UTF-8"));
        assert!(locale_value_is_utf8("C.utf8"));
        assert!(!locale_value_is_utf8("C"));
        assert!(!locale_value_is_utf8("POSIX"));
        assert!(!locale_value_is_utf8("de_DE.ISO-8859-1"));
    }
}
