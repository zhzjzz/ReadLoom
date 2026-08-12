use std::collections::HashMap;

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};

pub const DEFAULT_TXT_CHAPTER_PATTERN: &str = r"^(?:序章|楔子|正文|终章|后记|尾声|番外|第\s{0,4}[\d〇零一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+\s{0,4}(?:章|节|卷|集)|\d{1,6}[ \t　]+).{0,30}$";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AppTheme {
    #[serde(alias = "warm")]
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WindowCloseAction {
    Exit,
    Tray,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TextAlignment {
    #[serde(alias = "start")]
    Left,
    Justify,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TxtLeadingIndent {
    Clean,
    Preserve,
}

impl<'de> Deserialize<'de> for TxtLeadingIndent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum CompatibilityValue {
            Name(String),
            Legacy(bool),
        }
        Ok(match CompatibilityValue::deserialize(deserializer)? {
            CompatibilityValue::Name(value) if value == "preserve" => Self::Preserve,
            CompatibilityValue::Name(_) => Self::Clean,
            CompatibilityValue::Legacy(true) => Self::Clean,
            CompatibilityValue::Legacy(false) => Self::Preserve,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TxtBlankLines {
    Preserve,
    Single,
    Remove,
}

impl<'de> Deserialize<'de> for TxtBlankLines {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum CompatibilityValue {
            Name(String),
            Legacy(bool),
        }
        Ok(match CompatibilityValue::deserialize(deserializer)? {
            CompatibilityValue::Name(value) if value == "preserve" => Self::Preserve,
            CompatibilityValue::Name(value) if value == "remove" => Self::Remove,
            CompatibilityValue::Name(_) => Self::Single,
            CompatibilityValue::Legacy(true) => Self::Preserve,
            CompatibilityValue::Legacy(false) => Self::Remove,
        })
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChapterTitleStyle {
    #[serde(alias = "emphasize", alias = "center")]
    Prominent,
    Compact,
    #[serde(alias = "keep")]
    Plain,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ReadingSettings {
    /// Stable font option id, not a file path. Fonts are never downloaded by Readloom.
    pub font_family: String,
    pub font_size: i32,
    pub font_weight: i32,
    pub letter_spacing: f32,
    pub first_line_indent: f32,
    pub line_height: f32,
    pub paragraph_spacing: f32,
    pub content_width: i32,
    pub horizontal_margin: i32,
    pub vertical_margin: i32,
    pub text_alignment: TextAlignment,
    pub columns: i32,
}

impl Default for ReadingSettings {
    fn default() -> Self {
        Self {
            font_family: "source-han-serif".to_owned(),
            font_size: 19,
            font_weight: 400,
            letter_spacing: 0.0,
            first_line_indent: 2.0,
            line_height: 1.7,
            paragraph_spacing: 0.15,
            content_width: 780,
            horizontal_margin: 40,
            vertical_margin: 30,
            text_alignment: TextAlignment::Justify,
            columns: 1,
        }
    }
}

impl ReadingSettings {
    pub fn resolved_font_family(&self) -> &'static str {
        match self.font_family.as_str() {
            "source-han-serif" => "Source Han Serif SC",
            "noto-serif-cjk" => "Noto Serif CJK SC",
            "source-han-sans" => "Source Han Sans SC",
            "noto-sans-cjk" => "Noto Sans CJK SC",
            "lxgw-wenkai" => "LXGW WenKai",
            _ => "Microsoft YaHei UI",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct TxtSettings {
    pub leading_indent: TxtLeadingIndent,
    #[serde(alias = "preserveBlankLines")]
    pub blank_lines: TxtBlankLines,
    pub merge_wrapped_lines: bool,
    pub chapter_title_style: ChapterTitleStyle,
}

impl Default for TxtSettings {
    fn default() -> Self {
        Self {
            leading_indent: TxtLeadingIndent::Clean,
            blank_lines: TxtBlankLines::Single,
            merge_wrapped_lines: false,
            chapter_title_style: ChapterTitleStyle::Prominent,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct EpubSettings {
    #[serde(alias = "publisherStyles")]
    pub use_publisher_styles: bool,
    #[serde(alias = "overrideFonts")]
    pub override_font: bool,
    pub override_font_size: bool,
    pub override_indent: bool,
    pub override_line_height: bool,
    pub override_paragraph_spacing: bool,
    #[serde(alias = "allowEmbeddedFonts")]
    pub use_embedded_fonts: bool,
}

impl Default for EpubSettings {
    fn default() -> Self {
        Self {
            use_publisher_styles: true,
            override_font: false,
            override_font_size: true,
            override_indent: false,
            override_line_height: true,
            override_paragraph_spacing: false,
            use_embedded_fonts: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ShortcutSettings {
    pub open: String,
    pub save: String,
    pub save_as: String,
    pub close: String,
    pub toggle_edit: String,
    pub previous_chapter: String,
    pub next_chapter: String,
    pub bookmark: String,
    pub show_library: String,
    pub show_settings: String,
}

impl ShortcutSettings {
    pub const ACTIONS: [(&'static str, &'static str); 10] = [
        ("open", "打开文件"),
        ("save", "保存"),
        ("saveAs", "另存为"),
        ("close", "关闭当前图书"),
        ("toggleEdit", "切换编辑模式"),
        ("previousChapter", "上一章"),
        ("nextChapter", "下一章"),
        ("bookmark", "添加书签"),
        ("showLibrary", "打开书库"),
        ("showSettings", "打开设置"),
    ];

    pub fn get(&self, action: &str) -> Option<&str> {
        let value = match action {
            "open" => &self.open,
            "save" => &self.save,
            "saveAs" => &self.save_as,
            "close" => &self.close,
            "toggleEdit" => &self.toggle_edit,
            "previousChapter" => &self.previous_chapter,
            "nextChapter" => &self.next_chapter,
            "bookmark" => &self.bookmark,
            "showLibrary" => &self.show_library,
            "showSettings" => &self.show_settings,
            _ => return None,
        };
        Some(value)
    }

    pub fn set(&mut self, action: &str, shortcut: &str) -> bool {
        let target = match action {
            "open" => &mut self.open,
            "save" => &mut self.save,
            "saveAs" => &mut self.save_as,
            "close" => &mut self.close,
            "toggleEdit" => &mut self.toggle_edit,
            "previousChapter" => &mut self.previous_chapter,
            "nextChapter" => &mut self.next_chapter,
            "bookmark" => &mut self.bookmark,
            "showLibrary" => &mut self.show_library,
            "showSettings" => &mut self.show_settings,
            _ => return false,
        };
        *target = normalize_shortcut(shortcut);
        true
    }

    fn normalized(mut self) -> Self {
        for (action, _) in Self::ACTIONS {
            let value = self.get(action).unwrap_or_default().to_owned();
            self.set(action, &value);
        }
        self
    }

    fn conflict(&self) -> Option<String> {
        let mut used = HashMap::<String, &str>::new();
        for (action, label) in Self::ACTIONS {
            let shortcut = self.get(action).unwrap_or_default();
            if shortcut.is_empty() {
                continue;
            }
            let key = shortcut.to_ascii_lowercase();
            if let Some(previous) = used.insert(key, label) {
                return Some(format!(
                    "快捷键“{shortcut}”已同时分配给“{previous}”和“{label}”。"
                ));
            }
        }
        None
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct BookSettings {
    pub txt_chapter_pattern: String,
}

impl Default for BookSettings {
    fn default() -> Self {
        Self {
            txt_chapter_pattern: DEFAULT_TXT_CHAPTER_PATTERN.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DataSettings {
    pub backup_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub theme: AppTheme,
    pub library_columns: i32,
    pub background_opacity: f32,
    pub minimize_to_tray: bool,
    pub close_action: WindowCloseAction,
    pub reading: ReadingSettings,
    pub txt: TxtSettings,
    pub epub: EpubSettings,
    pub shortcuts: ShortcutSettings,
    pub books: BookSettings,
    pub data: DataSettings,
    /// Compatibility with the first native settings slice. Never written back.
    #[serde(rename = "closeToTray", skip_serializing, default)]
    pub(crate) legacy_close_to_tray: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: AppTheme::System,
            library_columns: 4,
            background_opacity: 0.2,
            minimize_to_tray: false,
            close_action: WindowCloseAction::Exit,
            reading: ReadingSettings::default(),
            txt: TxtSettings::default(),
            epub: EpubSettings::default(),
            shortcuts: ShortcutSettings::default(),
            books: BookSettings::default(),
            data: DataSettings::default(),
            legacy_close_to_tray: false,
        }
    }
}

impl AppSettings {
    pub(crate) fn normalized(mut self) -> Result<Self, String> {
        if self.legacy_close_to_tray {
            self.close_action = WindowCloseAction::Tray;
        }
        self.library_columns = if (3..=5).contains(&self.library_columns) {
            self.library_columns
        } else {
            4
        };
        self.background_opacity = finite_or(self.background_opacity, 0.2).clamp(0.0, 1.0);
        self.reading.font_family = normalize_font_id(&self.reading.font_family).to_owned();
        self.reading.font_size = self.reading.font_size.clamp(12, 36);
        self.reading.font_weight = self.reading.font_weight.clamp(300, 700);
        self.reading.letter_spacing = finite_or(self.reading.letter_spacing, 0.0).clamp(-0.05, 0.3);
        self.reading.first_line_indent =
            finite_or(self.reading.first_line_indent, 2.0).clamp(0.0, 4.0);
        self.reading.line_height = finite_or(self.reading.line_height, 1.7).clamp(1.2, 2.4);
        self.reading.paragraph_spacing =
            finite_or(self.reading.paragraph_spacing, 0.15).clamp(0.0, 1.5);
        self.reading.content_width = self.reading.content_width.clamp(480, 1280);
        self.reading.horizontal_margin = self.reading.horizontal_margin.clamp(8, 160);
        self.reading.vertical_margin = self.reading.vertical_margin.clamp(8, 120);
        self.reading.columns = if self.reading.columns == 2 { 2 } else { 1 };
        self.shortcuts = self.shortcuts.normalized();
        if let Some(message) = self.shortcuts.conflict() {
            return Err(message);
        }
        self.books.txt_chapter_pattern = self
            .books
            .txt_chapter_pattern
            .trim()
            .chars()
            .take(2048)
            .collect();
        if self.books.txt_chapter_pattern.is_empty() {
            return Err("TXT 章节识别正则不能为空。".to_owned());
        }
        Regex::new(&self.books.txt_chapter_pattern)
            .map_err(|error| format!("TXT 章节识别正则无效：{error}"))?;
        self.data.backup_path = self.data.backup_path.trim().chars().take(1024).collect();
        Ok(self)
    }
}

fn normalize_font_id(value: &str) -> &str {
    match value.trim() {
        "system" | "source-han-serif" | "noto-serif-cjk" | "source-han-sans" | "noto-sans-cjk"
        | "lxgw-wenkai" => value.trim(),
        "Microsoft YaHei UI" => "system",
        _ => "source-han-serif",
    }
}

fn normalize_shortcut(value: &str) -> String {
    value.trim().chars().take(48).collect()
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_settings_contract() {
        let settings = AppSettings::default();
        assert_eq!(settings.theme, AppTheme::System);
        assert_eq!(settings.library_columns, 4);
        assert_eq!(settings.background_opacity, 0.2);
        assert_eq!(settings.reading.font_size, 19);
        assert_eq!(settings.reading.first_line_indent, 2.0);
        assert_eq!(settings.reading.paragraph_spacing, 0.15);
        assert_eq!(settings.txt.blank_lines, TxtBlankLines::Single);
    }

    #[test]
    fn settings_normalization_clamps_untrusted_values() {
        let mut settings = AppSettings {
            library_columns: 99,
            background_opacity: f32::NAN,
            ..AppSettings::default()
        };
        settings.reading.font_size = -50;
        settings.reading.content_width = 9_999;
        settings.reading.font_family = "unknown-font".to_owned();

        let normalized = settings.normalized().expect("normalize");

        assert_eq!(normalized.library_columns, 4);
        assert_eq!(normalized.background_opacity, 0.2);
        assert_eq!(normalized.reading.font_size, 12);
        assert_eq!(normalized.reading.content_width, 1280);
        assert_eq!(normalized.reading.font_family, "source-han-serif");
    }

    #[test]
    fn duplicate_shortcuts_are_rejected() {
        let mut settings = AppSettings::default();
        settings.shortcuts.open = "Ctrl+O".to_owned();
        settings.shortcuts.save = "ctrl+o".to_owned();
        assert!(settings.normalized().unwrap_err().contains("同时分配"));
    }

    #[test]
    fn invalid_chapter_pattern_is_rejected() {
        let mut settings = AppSettings::default();
        settings.books.txt_chapter_pattern = "[".to_owned();
        assert!(settings.normalized().unwrap_err().contains("正则无效"));
    }

    #[test]
    fn first_native_settings_json_is_migrated_without_rejection() {
        let legacy = r#"{
            "theme":"warm","libraryColumns":5,"backgroundOpacity":1.0,
            "minimizeToTray":true,"closeToTray":true,
            "reading":{"fontFamily":"Microsoft YaHei UI","fontSize":23,"paragraphSpacing":14},
            "txt":{"leadingIndent":false,"preserveBlankLines":true,"chapterTitleStyle":"keep"},
            "epub":{"publisherStyles":false,"overrideFonts":true,"allowEmbeddedFonts":false}
        }"#;
        let migrated = serde_json::from_str::<AppSettings>(legacy)
            .unwrap()
            .normalized()
            .unwrap();
        assert_eq!(migrated.theme, AppTheme::Light);
        assert_eq!(migrated.close_action, WindowCloseAction::Tray);
        assert_eq!(migrated.reading.font_family, "system");
        assert_eq!(migrated.txt.leading_indent, TxtLeadingIndent::Preserve);
        assert_eq!(migrated.txt.blank_lines, TxtBlankLines::Preserve);
        assert_eq!(migrated.txt.chapter_title_style, ChapterTitleStyle::Plain);
        assert!(!migrated.epub.use_publisher_styles);
        assert!(migrated.epub.override_font);
        assert!(!migrated.epub.use_embedded_fonts);
    }
}
