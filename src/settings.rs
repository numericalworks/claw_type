//! Where to find Ollama, and which model to use.
//!
//! The settings live in a small `key = value` file under the platform's config
//! directory, so they survive restarts. On Unix the file is created readable
//! only by its owner, because the API key is a secret.

use std::path::{Path, PathBuf};

/// Where Ollama listens by default.
pub const DEFAULT_URL: &str = "http://localhost:11434";

/// How the app talks to Ollama.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The base URL of the server.
    pub url: String,
    /// An API key, for a hosted (cloud) server. Empty means none.
    pub api_key: String,
    /// The model to use, chosen from what the server offers.
    pub model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            url: DEFAULT_URL.to_owned(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}

impl Settings {
    /// The base URL with a scheme and without a trailing slash.
    ///
    /// `localhost:11434` is what people usually type, so a missing scheme is
    /// filled in. An empty URL falls back to [`DEFAULT_URL`].
    pub fn base_url(&self) -> String {
        let raw = self.url.trim();
        let raw = if raw.is_empty() { DEFAULT_URL } else { raw };
        let with_scheme = if raw.starts_with("http://") || raw.starts_with("https://") {
            raw.to_owned()
        } else {
            format!("http://{raw}")
        };
        with_scheme.trim_end_matches('/').to_owned()
    }

    /// Whether a model has been chosen.
    pub fn has_model(&self) -> bool {
        !self.model.trim().is_empty()
    }

    /// Read the settings, falling back to the defaults.
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        Self::load_from(&path)
    }

    /// Write the settings, ignoring any failure: they are a convenience, and a
    /// read-only disk should not stop the app working.
    pub fn save(&self) {
        if let Some(path) = config_path() {
            let _ = self.save_to(&path);
        }
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };

        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().to_owned();
            match key.trim() {
                "url" => settings.url = value,
                "api_key" => settings.api_key = value,
                "model" => settings.model = value,
                _ => {}
            }
        }
        settings
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = format!(
            "# claw_type settings\nurl = {}\napi_key = {}\nmodel = {}\n",
            self.url.trim(),
            self.api_key.trim(),
            self.model.trim(),
        );

        // Create it private from the start: the API key is a secret.
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)?;
            file.write_all(contents.as_bytes())?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            std::fs::write(path, contents)
        }
    }
}

/// The settings file: `$CLAW_TYPE_CONFIG`, or the platform's config directory.
pub fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CLAW_TYPE_CONFIG") {
        return Some(PathBuf::from(path));
    }
    Some(config_dir()?.join("claw_type").join("settings.conf"))
}

fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library").join("Application Support"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("claw_type_{}_{name}.conf", std::process::id()))
    }

    #[test]
    fn defaults_point_at_a_local_ollama() {
        let settings = Settings::default();
        assert_eq!(settings.url, DEFAULT_URL);
        assert!(settings.api_key.is_empty());
        assert!(!settings.has_model());
    }

    #[test]
    fn a_missing_scheme_is_filled_in() {
        let settings = Settings {
            url: "localhost:11434".to_owned(),
            ..Settings::default()
        };
        assert_eq!(settings.base_url(), "http://localhost:11434");
    }

    #[test]
    fn the_base_url_is_tidied_up() {
        for (given, expected) in [
            ("http://localhost:11434/", "http://localhost:11434"),
            ("  https://ollama.com  ", "https://ollama.com"),
            ("http://192.168.1.9:11434", "http://192.168.1.9:11434"),
            ("", DEFAULT_URL),
            ("   ", DEFAULT_URL),
        ] {
            let settings = Settings {
                url: given.to_owned(),
                ..Settings::default()
            };
            assert_eq!(settings.base_url(), expected, "{given:?}");
        }
    }

    #[test]
    fn settings_round_trip() {
        let path = temp_path("round_trip");
        let settings = Settings {
            url: "https://ollama.com".to_owned(),
            api_key: "sk-secret".to_owned(),
            model: "llama3.2:latest".to_owned(),
        };
        settings.save_to(&path).expect("save");
        assert_eq!(Settings::load_from(&path), settings);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        assert_eq!(Settings::load_from(&path), Settings::default());
    }

    #[test]
    fn unknown_and_malformed_lines_are_ignored() {
        let path = temp_path("malformed");
        std::fs::write(
            &path,
            "# a comment\nnonsense\nurl = http://example.test:1234\nmodel=\n",
        )
        .expect("write");

        let settings = Settings::load_from(&path);
        assert_eq!(settings.url, "http://example.test:1234");
        assert_eq!(settings.model, "");
        assert!(settings.api_key.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn the_settings_file_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_path("permissions");
        Settings {
            api_key: "sk-secret".to_owned(),
            ..Settings::default()
        }
        .save_to(&path)
        .expect("save");

        let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
        assert_eq!(mode & 0o077, 0, "mode was {:o}", mode & 0o777);
        let _ = std::fs::remove_file(&path);
    }
}
