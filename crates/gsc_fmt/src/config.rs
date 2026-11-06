//! The `[format]` table of `gruff.toml` (or `gsc.toml`).
//!
//! ```toml
//! game = "t7"                # optional; `[format] game` wins over the top-level key
//! [format]
//! indent = "tabs"            # or "spaces"
//! indent_width = 4           # spaces per level when indent = "spaces"
//! paren_spacing = "padded"   # or "compact", "preserve"; default per engine family
//! style = "full"             # or "minimal"
//! max_width = 100
//! brace_style = "next_line"  # or "same_line"
//! ```
//!
//! Every key is listed in `docs/configuration.md`.

use std::path::{Path, PathBuf};

use crate::{BraceStyle, Indent, KeywordSpace, Options, ParenSpacing, Style, Wrap};

/// The settings of a `gruff.toml` or `gsc.toml`. `None` means "use the default".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormatConfig {
    pub game: Option<String>,
    pub indent: Option<Indent>,
    pub paren_spacing: Option<ParenSpacing>,
    pub style: Option<Style>,
    pub max_width: Option<u16>,
    pub tab_width: Option<u8>,
    pub brace_style: Option<BraceStyle>,
    pub bracket_spacing: Option<ParenSpacing>,
    pub keyword_space: Option<KeywordSpace>,
    pub wrap: Option<Wrap>,
    pub max_blank_lines: Option<u8>,
}

/// An invalid `gruff.toml` or `gsc.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

const KEYS: &[&str] = &[
    "game",
    "indent",
    "indent_width",
    "paren_spacing",
    "style",
    "max_width",
    "tab_width",
    "brace_style",
    "bracket_spacing",
    "keyword_space",
    "wrap",
    "max_blank_lines",
];

/// Read `format.<key>` as one of `names` through `parse`.
fn named<T>(
    format: &toml::Table,
    key: &str,
    names: &str,
    parse: fn(&str) -> Option<T>,
) -> Result<Option<T>, ConfigError> {
    let Some(v) = format.get(key) else {
        return Ok(None);
    };
    let bad = || ConfigError(format!("`format.{key}` must be one of {names}"));
    let s = v.as_str().ok_or_else(bad)?;
    parse(s).map(Some).ok_or_else(bad)
}

/// Read `format.<key>` as an integer in `lo..=hi`.
fn int(format: &toml::Table, key: &str, lo: i64, hi: i64) -> Result<Option<i64>, ConfigError> {
    let Some(v) = format.get(key) else {
        return Ok(None);
    };
    v.as_integer()
        .filter(|w| (lo..=hi).contains(w))
        .map(Some)
        .ok_or_else(|| {
            ConfigError(format!(
                "`format.{key}` must be an integer from {lo} to {hi}"
            ))
        })
}

impl FormatConfig {
    /// Read the text of a `gruff.toml` or `gsc.toml`. Unknown keys of `[format]` are errors; other tables are
    /// ignored (they belong to other commands).
    pub fn from_toml_str(text: &str) -> Result<FormatConfig, ConfigError> {
        let doc: toml::Table = text
            .parse()
            .map_err(|e| ConfigError(format!("invalid TOML: {e}")))?;
        let str_of = |v: &toml::Value, key: &str| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| ConfigError(format!("`{key}` must be a string")))
        };
        let mut cfg = FormatConfig::default();
        if let Some(g) = doc.get("game") {
            cfg.game = Some(str_of(g, "game")?);
        }
        let Some(format) = doc.get("format") else {
            return Ok(cfg);
        };
        let format = format
            .as_table()
            .ok_or_else(|| ConfigError("`format` must be a table".to_owned()))?;
        if let Some(k) = format.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(ConfigError(format!(
                "unknown key `format.{k}`; known: {}",
                KEYS.join(", ")
            )));
        }
        if let Some(g) = format.get("game") {
            cfg.game = Some(str_of(g, "format.game")?);
        }
        let width = match format.get("indent_width") {
            None => 4,
            Some(w) => w
                .as_integer()
                .filter(|w| (1..=16).contains(w))
                .ok_or_else(|| {
                    ConfigError("`format.indent_width` must be an integer from 1 to 16".to_owned())
                })? as u8,
        };
        match format.get("indent").map(|v| str_of(v, "format.indent")) {
            None if format.contains_key("indent_width") => cfg.indent = Some(Indent::Spaces(width)),
            None => {}
            Some(v) => {
                cfg.indent = Some(match v?.as_str() {
                    "tabs" => Indent::Tabs,
                    "spaces" => Indent::Spaces(width),
                    other => {
                        return Err(ConfigError(format!(
                            "`format.indent` must be \"tabs\" or \"spaces\", not {other:?}"
                        )))
                    }
                })
            }
        }
        if let Some(v) = format.get("paren_spacing") {
            let v = str_of(v, "format.paren_spacing")?;
            cfg.paren_spacing = Some(ParenSpacing::from_name(&v).ok_or_else(|| {
                ConfigError(format!(
                    "`format.paren_spacing` must be \"padded\", \"compact\" or \"preserve\", not {v:?}"
                ))
            })?);
        }
        cfg.style = named(format, "style", "\"full\", \"minimal\"", Style::from_name)?;
        cfg.brace_style = named(
            format,
            "brace_style",
            "\"next_line\", \"same_line\"",
            BraceStyle::from_name,
        )?;
        cfg.bracket_spacing = named(
            format,
            "bracket_spacing",
            "\"padded\", \"compact\", \"preserve\"",
            ParenSpacing::from_name,
        )?;
        cfg.keyword_space = named(
            format,
            "keyword_space",
            "\"space\", \"none\", \"preserve\"",
            KeywordSpace::from_name,
        )?;
        cfg.wrap = named(
            format,
            "wrap",
            "\"fill\", \"one_per_line\"",
            Wrap::from_name,
        )?;
        cfg.max_width = int(format, "max_width", 20, 1000)?.map(|w| w as u16);
        cfg.tab_width = int(format, "tab_width", 1, 16)?.map(|w| w as u8);
        cfg.max_blank_lines = int(format, "max_blank_lines", 0, 10)?.map(|w| w as u8);
        Ok(cfg)
    }

    /// Read a `gruff.toml` or `gsc.toml` file.
    pub fn load(path: &Path) -> Result<FormatConfig, ConfigError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ConfigError(format!("cannot read {}: {e}", path.display())))?;
        FormatConfig::from_toml_str(&text)
            .map_err(|e| ConfigError(format!("{}: {}", path.display(), e.0)))
    }

    /// `base` with the settings of this file applied.
    pub fn apply(&self, base: Options) -> Options {
        Options {
            indent: self.indent.unwrap_or(base.indent),
            paren_spacing: self.paren_spacing.unwrap_or(base.paren_spacing),
            style: self.style.unwrap_or(base.style),
            max_width: self.max_width.unwrap_or(base.max_width),
            tab_width: self.tab_width.unwrap_or(base.tab_width),
            brace_style: self.brace_style.unwrap_or(base.brace_style),
            bracket_spacing: self.bracket_spacing.unwrap_or(base.bracket_spacing),
            keyword_space: self.keyword_space.unwrap_or(base.keyword_space),
            wrap: self.wrap.unwrap_or(base.wrap),
            max_blank_lines: self.max_blank_lines.unwrap_or(base.max_blank_lines),
        }
    }
}

/// The nearest `gruff.toml` or `gsc.toml` (the older name) in `start` or one of its ancestors;
/// `gruff.toml` wins in the same directory.
pub fn find_config(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .flat_map(|d| [d.join("gruff.toml"), d.join("gsc.toml")])
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_key() {
        let cfg = FormatConfig::from_toml_str(
            "game = \"t6\"\n[format]\nindent = \"spaces\"\nindent_width = 2\nparen_spacing = \"compact\"\n[check]\nx = 1\n",
        )
        .unwrap();
        assert_eq!(
            cfg,
            FormatConfig {
                game: Some("t6".to_owned()),
                indent: Some(Indent::Spaces(2)),
                paren_spacing: Some(ParenSpacing::Compact),
                ..FormatConfig::default()
            }
        );
        let opts = cfg.apply(Options::default());
        assert_eq!(opts.indent, Indent::Spaces(2));
    }

    #[test]
    fn empty_and_errors() {
        assert_eq!(
            FormatConfig::from_toml_str("").unwrap(),
            FormatConfig::default()
        );
        assert!(FormatConfig::from_toml_str("[format]\nwidth = 3").is_err());
        assert!(FormatConfig::from_toml_str("[format]\nindent = \"tab\"").is_err());
        assert!(FormatConfig::from_toml_str("[format]\nindent_width = 0").is_err());
        assert!(FormatConfig::from_toml_str("[format]\nparen_spacing = 1").is_err());
        assert!(FormatConfig::from_toml_str("[format").is_err());
        assert!(FormatConfig::from_toml_str("[format]\nstyle = \"pretty\"").is_err());
        assert!(FormatConfig::from_toml_str("[format]\nmax_width = 5").is_err());
    }

    #[test]
    fn reads_full_style_keys() {
        let cfg = FormatConfig::from_toml_str(
            "[format]\nstyle = \"minimal\"\nmax_width = 80\nbrace_style = \"same_line\"\nbracket_spacing = \"padded\"\nkeyword_space = \"none\"\nwrap = \"one_per_line\"\nmax_blank_lines = 2\ntab_width = 8\n",
        )
        .unwrap();
        let o = cfg.apply(Options::default());
        assert_eq!(o.style, Style::Minimal);
        assert_eq!(o.max_width, 80);
        assert_eq!(o.brace_style, BraceStyle::SameLine);
        assert_eq!(o.bracket_spacing, ParenSpacing::Padded);
        assert_eq!(o.keyword_space, KeywordSpace::None);
        assert_eq!(o.wrap, Wrap::OnePerLine);
        assert_eq!(o.max_blank_lines, 2);
        assert_eq!(o.tab_width, 8);
    }
}
