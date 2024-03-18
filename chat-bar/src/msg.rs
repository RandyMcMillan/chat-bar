use std::fmt::Display;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

pub(crate) static HOSTNAME: Lazy<String> =
    Lazy::new(|| hostname::get().unwrap().to_string_lossy().to_string());

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default)]
pub enum MsgKind {
    #[default]
    Chat,
    Join,
    Leave,
    System,
    Raw,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Msg {
    pub from: String,
    pub content: String,
    pub kind: MsgKind,
}

impl Default for Msg {
    fn default() -> Self {
        Self {
            from: HOSTNAME.clone(),
            content: "".to_string(),
            kind: MsgKind::Chat,
        }
    }
}

impl Msg {
    pub fn set_kind(mut self, kind: MsgKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn set_content(mut self, content: String) -> Self {
        self.content = content;
        self
    }

    pub fn new_self_chat(content: String) -> Self {
        Self {
            from: format!("{} (You)", HOSTNAME.clone()),
            content,
            kind: MsgKind::Chat,
        }
    }
}

impl<'a> From<&'a Msg> for ratatui::text::Line<'a> {
    fn from(m: &'a Msg) -> Self {
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::{Line, Span};
        use MsgKind::*;

        fn gen_color_by_hash(s: &str) -> Color {
            static LIGHT_COLORS: [Color; 6] = [
                Color::LightGreen,
                Color::LightYellow,
                Color::LightBlue,
                Color::LightMagenta,
                Color::LightCyan,
                Color::White,
            ];
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            s.hash(&mut hasher);
            let h = hasher.finish() as usize;
            return LIGHT_COLORS[h % LIGHT_COLORS.len()];
        }

        match m.kind {
            Join | Leave | System => Line::from(Span::styled(
                m.to_string(),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )),
            Chat => Line::default().spans(vec![
                Span::styled(
                    format!("{}: ", m.from),
                    Style::default().fg(gen_color_by_hash(&m.from)),
                ),
                m.content.clone().into(),
            ]),
            Raw => m.content.clone().into(),
        }
    }
}

impl Display for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            MsgKind::Join => write!(f, "{} joined", self.from),
            MsgKind::Leave => write!(f, "{} left", self.from),
            MsgKind::Chat => write!(f, "{}: {}", self.from, self.content),
            MsgKind::System => write!(f, "[System] {}", self.content),
            MsgKind::Raw => write!(f, "{}", self.content),
        }
    }
}
