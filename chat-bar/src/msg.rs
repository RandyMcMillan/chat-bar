use std::fmt::Display;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

pub(crate) static USER_NAME: Lazy<String> = Lazy::new(|| {
    format!(
        "{}",
        std::env::var("USER")
            .unwrap_or_else(|_| hostname::get().unwrap().to_string_lossy().to_string()),
    )
});

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
            from: USER_NAME.clone(),
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
}

impl<'a> From<&'a Msg> for ratatui::text::Line<'a> {
    fn from(m: &'a Msg) -> Self {
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::{Line, Span};
        use MsgKind::*;

        fn gen_color_by_hash(s: &str) -> Color {
            static LIGHT_COLORS: [Color; 5] = [
                Color::LightMagenta,
                Color::LightGreen,
                Color::LightYellow,
                Color::LightBlue,
                Color::LightCyan,
                // Color::White,
            ];
            let h = s.bytes().fold(0, |acc, b| acc ^ b as usize);
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
                    format!(
                        "{}{}: ",
                        m.from,
                        if m.from == *USER_NAME { " (You)" } else { "" }
                    ),
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
            MsgKind::Join => write!(f, "{} join", self.from),
            MsgKind::Leave => write!(f, "{} left", self.from),
            MsgKind::Chat => write!(f, "{}: {}", self.from, self.content),
            MsgKind::System => write!(f, "[System] {}", self.content),
            MsgKind::Raw => write!(f, "{}", self.content),
        }
    }
}
