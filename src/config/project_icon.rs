use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProjectIcon {
    #[default]
    None,
    Nerdfont,
    Emoji,
}

impl ProjectIcon {
    /// PROJECTS 行の開閉マーカー。None は従来の三角、その他はフォルダの開/閉。
    pub fn marker(self, expanded: bool) -> &'static str {
        match (self, expanded) {
            (ProjectIcon::None, true) => "▾",
            (ProjectIcon::None, false) => "▸",
            (ProjectIcon::Nerdfont, true) => "\u{f07c}", // nf-fa-folder_open
            (ProjectIcon::Nerdfont, false) => "\u{f07b}", // nf-fa-folder
            (ProjectIcon::Emoji, true) => "📂",
            (ProjectIcon::Emoji, false) => "📁",
        }
    }
}
