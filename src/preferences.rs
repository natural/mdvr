//! Revision-1 preferences storage. This module owns persistence data only;
//! session history, search state, and resource grants intentionally have no
//! representation here.

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

pub const PREFERENCES_VERSION: u16 = 1;
pub const PREFERENCES_FILE_NAME: &str = "preferences.json";
pub const MAX_PATH_BYTES: usize = 4 * 1024;
pub const MAX_THEME_NAME_BYTES: usize = 256;
pub const MIN_TEXT_SCALE_PERCENT: u16 = 50;
pub const MAX_TEXT_SCALE_PERCENT: u16 = 300;
pub const DEFAULT_TEXT_SCALE_PERCENT: u16 = 100;
pub const MIN_WINDOW_WIDTH: u32 = 320;
pub const MAX_WINDOW_WIDTH: u32 = 10_000;
pub const MIN_WINDOW_HEIGHT: u32 = 240;
pub const MAX_WINDOW_HEIGHT: u32 = 10_000;
pub const MIN_SCREEN_COORDINATE: i32 = -100_000;
pub const MAX_SCREEN_COORDINATE: i32 = 100_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScrollbarVisibility {
    Hide,
    Show,
    #[default]
    ShowOnScroll,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreferencesError {
    Io(String),
    Json(String),
    Invalid(&'static str),
    InvalidPath,
    InvalidTextScale(u16),
    InvalidWindow,
}

impl std::fmt::Display for PreferencesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PreferencesError {}

impl From<io::Error> for PreferencesError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadingLocator {
    pub heading: Option<String>,
    pub block: String,
    pub offset: u32,
}

impl ReadingLocator {
    pub fn validate(&self) -> Result<(), PreferencesError> {
        validate_optional_text(self.heading.as_deref(), "heading")?;
        validate_text(&self.block, "block", MAX_PATH_BYTES)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            x: 100,
            y: 100,
            width: 960,
            height: 700,
        }
    }
}

impl WindowGeometry {
    pub fn validate(self) -> Result<Self, PreferencesError> {
        if !(MIN_SCREEN_COORDINATE..=MAX_SCREEN_COORDINATE).contains(&self.x)
            || !(MIN_SCREEN_COORDINATE..=MAX_SCREEN_COORDINATE).contains(&self.y)
            || !(MIN_WINDOW_WIDTH..=MAX_WINDOW_WIDTH).contains(&self.width)
            || !(MIN_WINDOW_HEIGHT..=MAX_WINDOW_HEIGHT).contains(&self.height)
        {
            return Err(PreferencesError::InvalidWindow);
        }
        Ok(self)
    }

    pub fn bounded(self) -> Self {
        Self {
            x: self.x.clamp(MIN_SCREEN_COORDINATE, MAX_SCREEN_COORDINATE),
            y: self.y.clamp(MIN_SCREEN_COORDINATE, MAX_SCREEN_COORDINATE),
            width: self.width.clamp(MIN_WINDOW_WIDTH, MAX_WINDOW_WIDTH),
            height: self.height.clamp(MIN_WINDOW_HEIGHT, MAX_WINDOW_HEIGHT),
        }
    }

    /// Move saved geometry onto first usable display when its old display is gone.
    pub fn restore_on(self, displays: &[DisplayBounds]) -> Self {
        let saved = self.bounded();
        let Some(display) = displays.iter().copied().find(|display| display.is_usable()) else {
            return saved;
        };
        if displays.iter().any(|display| display.overlaps(saved)) {
            return saved;
        }

        let width = saved.width.min(display.width.max(MIN_WINDOW_WIDTH));
        let height = saved.height.min(display.height.max(MIN_WINDOW_HEIGHT));
        Self {
            x: display.x + ((display.width as i64 - width as i64).max(0) / 2) as i32,
            y: display.y + ((display.height as i64 - height as i64).max(0) / 2) as i32,
            width,
            height,
        }
        .bounded()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl DisplayBounds {
    fn is_usable(self) -> bool {
        self.width > 0 && self.height > 0
    }

    fn overlaps(self, window: WindowGeometry) -> bool {
        self.is_usable()
            && i64::from(window.x) < i64::from(self.x) + i64::from(self.width)
            && i64::from(self.x) < i64::from(window.x) + i64::from(window.width)
            && i64::from(window.y) < i64::from(self.y) + i64::from(self.height)
            && i64::from(self.y) < i64::from(window.y) + i64::from(window.height)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub version: u16,
    #[serde(default)]
    pub browsing_root: Option<PathBuf>,
    #[serde(default)]
    pub last_document: Option<PathBuf>,
    #[serde(default)]
    pub reading_locator: Option<ReadingLocator>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub theme_file: Option<PathBuf>,
    #[serde(default = "default_use_zed_config")]
    pub use_zed_config: bool,
    #[serde(default, alias = "toolbar_visibility")]
    pub scrollbar_visibility: ScrollbarVisibility,
    #[serde(default = "default_text_scale")]
    pub text_scale_percent: u16,
    #[serde(default)]
    pub window: WindowGeometry,
}

fn default_use_zed_config() -> bool {
    true
}

fn default_text_scale() -> u16 {
    DEFAULT_TEXT_SCALE_PERCENT
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: PREFERENCES_VERSION,
            browsing_root: None,
            last_document: None,
            reading_locator: None,
            theme: None,
            theme_file: None,
            use_zed_config: true,
            scrollbar_visibility: ScrollbarVisibility::default(),
            text_scale_percent: DEFAULT_TEXT_SCALE_PERCENT,
            window: WindowGeometry::default(),
        }
    }
}

impl Preferences {
    pub fn validate(&self) -> Result<(), PreferencesError> {
        if self.version != PREFERENCES_VERSION {
            return Err(PreferencesError::Invalid("unsupported preferences version"));
        }
        validate_optional_path(self.browsing_root.as_deref())?;
        validate_optional_path(self.last_document.as_deref())?;
        if let Some(locator) = &self.reading_locator {
            locator.validate()?;
        }
        validate_optional_text(self.theme.as_deref(), "theme")?;
        validate_optional_path(self.theme_file.as_deref())?;
        if !(MIN_TEXT_SCALE_PERCENT..=MAX_TEXT_SCALE_PERCENT).contains(&self.text_scale_percent) {
            return Err(PreferencesError::InvalidTextScale(self.text_scale_percent));
        }
        self.window.validate()?;
        Ok(())
    }

    pub fn set_text_scale(&mut self, percent: u16) -> Result<(), PreferencesError> {
        if !(MIN_TEXT_SCALE_PERCENT..=MAX_TEXT_SCALE_PERCENT).contains(&percent) {
            return Err(PreferencesError::InvalidTextScale(percent));
        }
        self.text_scale_percent = percent;
        Ok(())
    }

    pub fn set_window(&mut self, geometry: WindowGeometry) -> Result<(), PreferencesError> {
        self.window = geometry.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadOutcome {
    pub preferences: Preferences,
    pub recovered: bool,
}

pub fn application_support_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("mdvr")
        .join(PREFERENCES_FILE_NAME)
}

pub fn conventional_path() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| application_support_path(Path::new(&home)))
}

pub fn load(path: &Path) -> Result<Preferences, PreferencesError> {
    let bytes = fs::read(path).map_err(PreferencesError::from)?;
    let preferences: Preferences = serde_json::from_slice(&bytes)
        .map_err(|error| PreferencesError::Json(error.to_string()))?;
    preferences.validate()?;
    Ok(preferences)
}

/// Invalid or old state becomes defaults. Caller can show recovery UI when `recovered` is true.
pub fn load_or_default(path: &Path) -> LoadOutcome {
    match load(path) {
        Ok(preferences) => LoadOutcome {
            preferences,
            recovered: false,
        },
        Err(_) => LoadOutcome {
            preferences: Preferences::default(),
            recovered: path.exists(),
        },
    }
}

pub fn save(path: &Path, preferences: &Preferences) -> Result<(), PreferencesError> {
    preferences.validate()?;
    let bytes = serde_json::to_vec_pretty(preferences)
        .map_err(|error| PreferencesError::Json(error.to_string()))?;
    atomic_write(path, &bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), PreferencesError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        PREFERENCES_FILE_NAME,
        std::process::id(),
        nonce
    ));

    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok::<(), io::Error>(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(PreferencesError::from)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchIntent {
    Bare,
    ExplicitFile,
    ExplicitDirectory,
    FinderFile,
    Dock,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchState {
    pub browsing_root: Option<PathBuf>,
    pub document: Option<PathBuf>,
    pub reading_locator: Option<ReadingLocator>,
}

impl LaunchState {
    fn empty() -> Self {
        Self {
            browsing_root: None,
            document: None,
            reading_locator: None,
        }
    }
}

/// Restore is intentionally exclusive to Dock. Bare launch and every explicit
/// path discard stored document/root state instead of silently reopening it.
pub fn resolve_launch(
    intent: LaunchIntent,
    explicit_path: Option<&Path>,
    preferences: &Preferences,
    restored_document_available: bool,
) -> Result<LaunchState, PreferencesError> {
    match intent {
        LaunchIntent::Dock => Ok(LaunchState {
            browsing_root: preferences.browsing_root.clone(),
            document: restored_document_available
                .then(|| preferences.last_document.clone())
                .flatten(),
            reading_locator: restored_document_available
                .then(|| preferences.reading_locator.clone())
                .flatten(),
        }),
        LaunchIntent::ExplicitFile | LaunchIntent::FinderFile => {
            let path = required_absolute_path(explicit_path)?;
            Ok(LaunchState {
                browsing_root: path.parent().map(Path::to_path_buf),
                document: Some(path),
                reading_locator: None,
            })
        }
        LaunchIntent::ExplicitDirectory => {
            let path = required_absolute_path(explicit_path)?;
            Ok(LaunchState {
                browsing_root: Some(path),
                document: None,
                reading_locator: None,
            })
        }
        LaunchIntent::Bare => Ok(LaunchState::empty()),
    }
}

fn required_absolute_path(path: Option<&Path>) -> Result<PathBuf, PreferencesError> {
    let path = path.ok_or(PreferencesError::Invalid("explicit path missing"))?;
    validate_path(path)?;
    Ok(path.to_path_buf())
}

fn validate_optional_path(path: Option<&Path>) -> Result<(), PreferencesError> {
    path.map_or(Ok(()), validate_path)
}

fn validate_path(path: &Path) -> Result<(), PreferencesError> {
    if !path.is_absolute() || path.as_os_str().len() > MAX_PATH_BYTES {
        return Err(PreferencesError::InvalidPath);
    }
    path.to_str()
        .filter(|value| !value.chars().any(char::is_control))
        .map(|_| ())
        .ok_or(PreferencesError::InvalidPath)
}

fn validate_optional_text(
    value: Option<&str>,
    field: &'static str,
) -> Result<(), PreferencesError> {
    value.map_or(Ok(()), |value| {
        validate_text(value, field, MAX_THEME_NAME_BYTES)
    })
}

fn validate_text(value: &str, field: &'static str, max: usize) -> Result<(), PreferencesError> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(PreferencesError::Invalid(field));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mdvr-{name}-{}", std::process::id()))
    }

    #[test]
    fn round_trip_and_atomic_failure_keep_previous_settings() {
        let path = temp_path("preferences-round-trip.json");
        let _ = fs::remove_file(&path);
        let mut preferences = Preferences::default();
        preferences.browsing_root = Some(PathBuf::from("/work"));
        save(&path, &preferences).unwrap();
        assert_eq!(load(&path).unwrap(), preferences);

        let bad_path = path.join("cannot-write");
        assert!(save(&bad_path, &preferences).is_err());
        assert_eq!(load(&path).unwrap(), preferences);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn corrupt_state_recovers_without_panicking() {
        let path = temp_path("preferences-corrupt.json");
        fs::write(&path, b"{not json").unwrap();
        let outcome = load_or_default(&path);
        assert!(outcome.recovered);
        assert_eq!(outcome.preferences, Preferences::default());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn explicit_paths_beat_restore_and_missing_dock_document_keeps_root() {
        let preferences = Preferences {
            browsing_root: Some(PathBuf::from("/saved")),
            last_document: Some(PathBuf::from("/saved/readme.md")),
            reading_locator: Some(ReadingLocator {
                heading: Some("intro".into()),
                block: "p-1".into(),
                offset: 2,
            }),
            ..Preferences::default()
        };
        let explicit = resolve_launch(
            LaunchIntent::ExplicitFile,
            Some(Path::new("/chosen/doc.md")),
            &preferences,
            true,
        )
        .unwrap();
        assert_eq!(explicit.document, Some(PathBuf::from("/chosen/doc.md")));
        assert_eq!(explicit.browsing_root, Some(PathBuf::from("/chosen")));
        assert!(explicit.reading_locator.is_none());

        let missing = resolve_launch(LaunchIntent::Dock, None, &preferences, false).unwrap();
        assert_eq!(missing.browsing_root, Some(PathBuf::from("/saved")));
        assert!(missing.document.is_none());
        assert!(missing.reading_locator.is_none());
    }

    #[test]
    fn scale_and_geometry_are_bounded_and_removed_display_is_recovered() {
        assert!(Preferences::default().set_text_scale(301).is_err());
        let geometry = WindowGeometry {
            x: 20_000,
            y: 20_000,
            width: 10,
            height: 10,
        };
        assert!(geometry.validate().is_err());
        let recovered = WindowGeometry::default().restore_on(&[DisplayBounds {
            x: 0,
            y: 0,
            width: 1440,
            height: 900,
        }]);
        assert_eq!(recovered, WindowGeometry::default());
    }
}
