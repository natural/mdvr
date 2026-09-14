//! Validated reader appearance and theme importer.
//!
//! Imported values are reduced to colors and closed syntax roles. No CSS,
//! JavaScript, editor command, or arbitrary style value leaves this module.

use std::{fs, path::Path};

use serde_json::{Map, Value};

pub const MAX_THEME_BYTES: usize = 1024 * 1024;
pub const MAX_FAMILY_MEMBERS: usize = 64;
pub const MAX_SYNTAX_TOKENS: usize = 128;
pub const MAX_NAME_BYTES: usize = 256;
pub const MIN_SCALE_PERCENT: u16 = 50;
pub const MAX_SCALE_PERCENT: u16 = 300;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThemeError {
    Io(String),
    Json(String),
    Invalid(&'static str),
    InvalidValue(String),
    Contract(String),
    TooManyMembers,
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ThemeError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppearanceMode {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxRole {
    Keyword,
    String,
    Comment,
    Number,
    Function,
    Type,
    Operator,
    Punctuation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxToken {
    pub role: SyntaxRole,
    pub foreground: String,
    pub background: Option<String>,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppearanceTokens {
    pub mode: AppearanceMode,
    pub scale_percent: u16,
    pub reader_background: String,
    pub reader_foreground: String,
    pub code_background: String,
    pub accent: String,
    pub syntax: Vec<SyntaxToken>,
}

impl AppearanceTokens {
    pub fn validate(&self) -> Result<(), ThemeError> {
        if !(MIN_SCALE_PERCENT..=MAX_SCALE_PERCENT).contains(&self.scale_percent) {
            return Err(ThemeError::Invalid("scale"));
        }
        for color in [
            &self.reader_background,
            &self.reader_foreground,
            &self.code_background,
            &self.accent,
        ] {
            validate_color(color)?;
        }
        if self.syntax.len() > MAX_SYNTAX_TOKENS {
            return Err(ThemeError::Invalid("syntax token count"));
        }
        for token in &self.syntax {
            validate_color(&token.foreground)?;
            if let Some(background) = &token.background {
                validate_color(background)?;
            }
        }
        Ok(())
    }

    pub fn with_scale(mut self, scale_percent: u16) -> Result<Self, ThemeError> {
        if !(MIN_SCALE_PERCENT..=MAX_SCALE_PERCENT).contains(&scale_percent) {
            return Err(ThemeError::InvalidValue(scale_percent.to_string()));
        }
        self.scale_percent = scale_percent;
        Ok(self)
    }

    pub fn as_revision_one(&self) -> Result<crate::contracts::Appearance, ThemeError> {
        self.validate()?;
        let appearance = crate::contracts::Appearance {
            mode: match self.mode {
                AppearanceMode::Light => crate::contracts::AppearanceMode::Light,
                AppearanceMode::Dark => crate::contracts::AppearanceMode::Dark,
            },
            scale_percent: self.scale_percent,
            reader_background: self.reader_background.clone(),
            reader_foreground: self.reader_foreground.clone(),
            code_background: self.code_background.clone(),
            accent: self.accent.clone(),
            syntax: self
                .syntax
                .iter()
                .map(|token| crate::contracts::SyntaxToken {
                    role: match token.role {
                        SyntaxRole::Keyword => crate::contracts::SyntaxRole::Keyword,
                        SyntaxRole::String => crate::contracts::SyntaxRole::String,
                        SyntaxRole::Comment => crate::contracts::SyntaxRole::Comment,
                        SyntaxRole::Number => crate::contracts::SyntaxRole::Number,
                        SyntaxRole::Function => crate::contracts::SyntaxRole::Function,
                        SyntaxRole::Type => crate::contracts::SyntaxRole::Type,
                        SyntaxRole::Operator => crate::contracts::SyntaxRole::Operator,
                        SyntaxRole::Punctuation => crate::contracts::SyntaxRole::Punctuation,
                    },
                    foreground: token.foreground.clone(),
                    background: token.background.clone(),
                    bold: token.bold,
                    italic: token.italic,
                })
                .collect(),
        };
        crate::contracts::encode(&crate::contracts::Envelope::new(
            crate::contracts::Message::AppearanceUpdate(crate::contracts::AppearanceUpdate {
                document: None,
                appearance: appearance.clone(),
            }),
        ))
        .map_err(|error| ThemeError::Contract(error.to_string()))?;
        Ok(appearance)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Theme {
    pub name: String,
    pub tokens: AppearanceTokens,
}

impl Theme {
    pub fn validate(&self) -> Result<(), ThemeError> {
        validate_name(&self.name)?;
        self.tokens.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThemeFamily {
    pub name: String,
    pub members: Vec<Theme>,
}

impl ThemeFamily {
    pub fn validate(&self) -> Result<(), ThemeError> {
        validate_name(&self.name)?;
        if self.members.is_empty() || self.members.len() > MAX_FAMILY_MEMBERS {
            return Err(ThemeError::Invalid("theme family members"));
        }
        for member in &self.members {
            member.validate()?;
        }
        Ok(())
    }

    pub fn member(&self, name: &str) -> Option<&Theme> {
        self.members.iter().find(|member| member.name == name)
    }

    pub fn sort_members(&mut self) {
        self.members.sort_by(|left, right| {
            let mode = |theme: &Theme| match theme.tokens.mode {
                AppearanceMode::Light => 0,
                AppearanceMode::Dark => 1,
            };
            mode(left).cmp(&mode(right)).then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
        });
    }
}

pub fn default_theme(mode: AppearanceMode) -> Theme {
    let dark = mode == AppearanceMode::Dark;
    let tokens = if dark {
        AppearanceTokens {
            mode,
            scale_percent: 100,
            reader_background: "#1e1e1e".into(),
            reader_foreground: "#d4d4d4".into(),
            code_background: "#252526".into(),
            accent: "#569cd6".into(),
            syntax: default_dark_syntax(),
        }
    } else {
        AppearanceTokens {
            mode,
            scale_percent: 100,
            reader_background: "#ffffff".into(),
            reader_foreground: "#242424".into(),
            code_background: "#f4f4f4".into(),
            accent: "#005cc5".into(),
            syntax: default_light_syntax(),
        }
    };
    Theme {
        name: if dark {
            "Default Dark Theme"
        } else {
            "Default Light Theme"
        }
        .into(),
        tokens,
    }
}

pub fn default_family() -> ThemeFamily {
    ThemeFamily {
        name: "Built-in themes".into(),
        members: vec![
            default_theme(AppearanceMode::Light),
            default_theme(AppearanceMode::Dark),
            tokyo_theme(AppearanceMode::Light),
            tokyo_theme(AppearanceMode::Dark),
        ],
    }
}

fn tokyo_theme(mode: AppearanceMode) -> Theme {
    let dark = mode == AppearanceMode::Dark;
    Theme {
        name: if dark {
            "Tokyo Dark Theme"
        } else {
            "Tokyo Light Theme"
        }
        .into(),
        tokens: AppearanceTokens {
            mode,
            scale_percent: 100,
            reader_background: if dark { "#1a1b26" } else { "#e6e7ed" }.into(),
            reader_foreground: if dark { "#c0caf5" } else { "#3760bf" }.into(),
            code_background: if dark { "#16161e" } else { "#d5d6db" }.into(),
            accent: if dark { "#7aa2f7" } else { "#2e7de9" }.into(),
            syntax: if dark {
                default_dark_syntax()
            } else {
                default_light_syntax()
            },
        },
    }
}

pub fn import_family(source: &str) -> Result<ThemeFamily, ThemeError> {
    if source.len() > MAX_THEME_BYTES {
        return Err(ThemeError::Invalid("theme too large"));
    }
    let value: Value =
        serde_json::from_str(source).map_err(|error| ThemeError::Json(error.to_string()))?;
    let root = value.as_object().ok_or(ThemeError::Invalid("theme root"))?;
    let family_name = optional_name(root.get("name"))?.unwrap_or_else(|| "Imported themes".into());
    let members = root
        .get("themes")
        .and_then(Value::as_array)
        .ok_or(ThemeError::Invalid("themes array"))?;
    if members.is_empty() || members.len() > MAX_FAMILY_MEMBERS {
        return Err(ThemeError::TooManyMembers);
    }

    let members = members
        .iter()
        .map(parse_theme)
        .collect::<Result<Vec<_>, _>>()?;
    let family = ThemeFamily {
        name: family_name,
        members,
    };
    family.validate()?;
    Ok(family)
}

pub fn import_file(path: &Path) -> Result<ThemeFamily, ThemeError> {
    let bytes = fs::read(path).map_err(|error| ThemeError::Io(error.to_string()))?;
    if bytes.len() > MAX_THEME_BYTES {
        return Err(ThemeError::Invalid("theme too large"));
    }
    let source = std::str::from_utf8(&bytes).map_err(|_| ThemeError::Invalid("theme encoding"))?;
    import_family(source)
}

/// Parse first, then replace. Invalid input cannot alter active appearance.
pub fn apply_import(current: &mut Theme, source: &str) -> Result<ThemeFamily, ThemeError> {
    let family = import_family(source)?;
    let replacement = family
        .members
        .first()
        .cloned()
        .ok_or(ThemeError::Invalid("empty theme family"))?;
    *current = replacement;
    Ok(family)
}

pub fn switch_family_member(
    current: &mut Theme,
    family: &ThemeFamily,
    name: &str,
) -> Result<(), ThemeError> {
    let replacement = family
        .member(name)
        .cloned()
        .ok_or(ThemeError::Invalid("unknown theme member"))?;
    replacement.validate()?;
    *current = replacement;
    Ok(())
}

fn parse_theme(value: &Value) -> Result<Theme, ThemeError> {
    let object = value
        .as_object()
        .ok_or(ThemeError::Invalid("theme member"))?;
    let name = required_name(object.get("name"))?;
    let mode = match object.get("appearance").and_then(Value::as_str) {
        Some("light") => AppearanceMode::Light,
        Some("dark") => AppearanceMode::Dark,
        Some(_) => return Err(ThemeError::Invalid("theme appearance")),
        None => return Err(ThemeError::Invalid("theme appearance")),
    };
    let style = object
        .get("style")
        .and_then(Value::as_object)
        .ok_or(ThemeError::Invalid("theme style"))?;

    let reader_background = required_color(style, &["background", "editor.background"])?;
    let reader_foreground = required_color(style, &["foreground", "text", "editor.foreground"])?;
    let code_background = optional_color(
        style,
        &[
            "code.background",
            "editor.background",
            "terminal.background",
        ],
    )?
    .unwrap_or_else(|| reader_background.clone());
    let accent = optional_color(
        style,
        &[
            "accent",
            "text.accent",
            "link_text",
            "link_text.hover",
            "editor.link_text",
            "editor.active_line.foreground",
        ],
    )?
    .unwrap_or_else(|| reader_foreground.clone());
    let syntax = parse_syntax(style.get("syntax"))?;

    let theme = Theme {
        name,
        tokens: AppearanceTokens {
            mode,
            scale_percent: 100,
            reader_background,
            reader_foreground,
            code_background,
            accent,
            syntax,
        },
    };
    theme.validate()?;
    Ok(theme)
}

fn parse_syntax(value: Option<&Value>) -> Result<Vec<SyntaxToken>, ThemeError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let syntax = value.as_object().ok_or(ThemeError::Invalid("syntax map"))?;
    let mut tokens = Vec::new();
    for (name, value) in syntax {
        let Some(role) = syntax_role(name) else {
            continue;
        };
        let (foreground, bold, italic) = match value {
            Value::String(color) => (color.clone(), false, false),
            Value::Object(object) => {
                let color = object
                    .get("color")
                    .and_then(Value::as_str)
                    .ok_or(ThemeError::Invalid("syntax color"))?;
                let (bold, italic) = font_style(object.get("font_style"))?;
                (color.to_owned(), bold, italic)
            }
            _ => return Err(ThemeError::Invalid("syntax token")),
        };
        let foreground = normalize_color(&foreground)?;
        tokens.push(SyntaxToken {
            role,
            foreground,
            background: None,
            bold,
            italic,
        });
    }
    if tokens.len() > MAX_SYNTAX_TOKENS {
        return Err(ThemeError::Invalid("syntax token count"));
    }
    Ok(tokens)
}

fn syntax_role(name: &str) -> Option<SyntaxRole> {
    Some(match name {
        "keyword" => SyntaxRole::Keyword,
        "string" => SyntaxRole::String,
        "comment" => SyntaxRole::Comment,
        "number" => SyntaxRole::Number,
        "function" => SyntaxRole::Function,
        "type" => SyntaxRole::Type,
        "operator" => SyntaxRole::Operator,
        "punctuation" => SyntaxRole::Punctuation,
        _ => return None,
    })
}

fn font_style(value: Option<&Value>) -> Result<(bool, bool), ThemeError> {
    let Some(value) = value else {
        return Ok((false, false));
    };
    let Some(style) = value.as_str() else {
        if value.is_null() {
            return Ok((false, false));
        }
        return Err(ThemeError::Invalid("syntax font style"));
    };
    match style {
        "normal" | "" => Ok((false, false)),
        "bold" => Ok((true, false)),
        "italic" => Ok((false, true)),
        "bold italic" | "italic bold" => Ok((true, true)),
        _ => Err(ThemeError::Invalid("syntax font style")),
    }
}

fn required_color(style: &Map<String, Value>, keys: &[&str]) -> Result<String, ThemeError> {
    optional_color(style, keys)?.ok_or(ThemeError::Invalid("required theme color"))
}

fn optional_color(style: &Map<String, Value>, keys: &[&str]) -> Result<Option<String>, ThemeError> {
    for key in keys {
        let Some(value) = style.get(*key) else {
            continue;
        };
        let Some(color) = value.as_str() else {
            if value.is_null() {
                continue;
            }
            return Err(ThemeError::Invalid("theme color type"));
        };
        return Ok(Some(normalize_color(color)?));
    }
    Ok(None)
}

fn validate_color(value: &str) -> Result<(), ThemeError> {
    normalize_color(value).map(|_| ())
}

fn normalize_color(value: &str) -> Result<String, ThemeError> {
    let bytes = value.as_bytes();
    let valid_hex = |bytes: &[u8]| bytes.iter().all(u8::is_ascii_hexdigit);
    match bytes.len() {
        7 if value.starts_with('#') && valid_hex(&bytes[1..]) => Ok(value.to_owned()),
        // Accept opaque six-digit RGB values with optional FF alpha.
        9 if value.starts_with('#')
            && valid_hex(&bytes[1..])
            && bytes[7..].eq_ignore_ascii_case(b"ff") =>
        {
            Ok(value[..7].to_owned())
        }
        _ => Err(ThemeError::InvalidValue(value.to_owned())),
    }
}

fn required_name(value: Option<&Value>) -> Result<String, ThemeError> {
    value
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(ThemeError::Invalid("theme name"))
        .and_then(|name| {
            validate_name(&name)?;
            Ok(name)
        })
}

fn optional_name(value: Option<&Value>) -> Result<Option<String>, ThemeError> {
    value.map(|value| required_name(Some(value))).transpose()
}

fn validate_name(name: &str) -> Result<(), ThemeError> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES || name.chars().any(char::is_control) {
        return Err(ThemeError::Invalid("theme name"));
    }
    Ok(())
}

fn default_light_syntax() -> Vec<SyntaxToken> {
    vec![
        token(SyntaxRole::Keyword, "#a626a4"),
        token(SyntaxRole::String, "#50a14f"),
        token(SyntaxRole::Comment, "#a0a1a7"),
        token(SyntaxRole::Number, "#986801"),
        token(SyntaxRole::Function, "#4078f2"),
        token(SyntaxRole::Type, "#c18401"),
    ]
}

fn default_dark_syntax() -> Vec<SyntaxToken> {
    vec![
        token(SyntaxRole::Keyword, "#c586c0"),
        token(SyntaxRole::String, "#ce9178"),
        token(SyntaxRole::Comment, "#6a9955"),
        token(SyntaxRole::Number, "#b5cea8"),
        token(SyntaxRole::Function, "#dcdcaa"),
        token(SyntaxRole::Type, "#4ec9b0"),
    ]
}

fn token(role: SyntaxRole, foreground: &str) -> SyntaxToken {
    SyntaxToken {
        role,
        foreground: foreground.into(),
        background: None,
        bold: false,
        italic: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMPORTED_THEME: &str = r##"{
        "name": "Safe family",
        "themes": [{
            "name": "Safe dark",
            "appearance": "dark",
            "style": {
                "background": "#111111",
                "foreground": "#eeeeee",
                "editor.background": "#222222",
                "accent": "#66aaff",
                "syntax": {
                    "keyword": "#ff66aa",
                    "comment": {"color": "#88aa88", "font_style": "italic"},
                    "editor_only": "url(javascript:bad)"
                },
                "editor_only": "<script>alert(1)</script>"
            }
        }]
    }"##;

    #[test]
    fn imports_family_and_maps_only_closed_tokens() {
        let family = import_family(IMPORTED_THEME).unwrap();
        assert_eq!(family.members.len(), 1);
        assert_eq!(family.members[0].tokens.syntax.len(), 2);
        assert_eq!(family.members[0].tokens.code_background, "#222222");
    }

    #[test]
    fn malicious_known_values_are_rejected_and_current_theme_survives() {
        let mut current = default_theme(AppearanceMode::Light);
        let before = current.clone();
        let bad = IMPORTED_THEME.replace("#111111", "url(javascript:bad)");
        assert!(apply_import(&mut current, &bad).is_err());
        assert_eq!(current, before);
    }

    #[test]
    fn family_switch_and_bounds_work() {
        let family = default_family();
        let mut current = family.members[0].clone();
        switch_family_member(&mut current, &family, &family.members[1].name).unwrap();
        assert_eq!(current.tokens.mode, AppearanceMode::Dark);
        assert!(current.tokens.clone().with_scale(301).is_err());
        assert!(import_family(&"x".repeat(MAX_THEME_BYTES + 1)).is_err());
    }

    #[test]
    fn revision_one_appearance_is_contract_validated() {
        let appearance = default_theme(AppearanceMode::Dark)
            .tokens
            .as_revision_one()
            .unwrap();
        assert_eq!(appearance.mode, crate::contracts::AppearanceMode::Dark);
        assert_eq!(appearance.reader_background, "#1e1e1e");
    }
}
