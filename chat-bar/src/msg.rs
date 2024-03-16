use std::fmt::Display;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

static HOSTNAME: Lazy<String> =
    Lazy::new(|| hostname::get().unwrap().to_string_lossy().to_string());

#[derive(Debug, Serialize, Deserialize, Default)]
pub enum MsgKind {
    #[default]
    Chat,
    Join,
    Leave,
    System
}

#[derive(Debug, Serialize, Deserialize)]
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
}

impl Display for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            MsgKind::Join => write!(f, "{} joined", self.from),
            MsgKind::Leave => write!(f, "{} left", self.from),
            MsgKind::Chat => write!(f, "{}: {}", self.from, self.content),
            MsgKind::System => write!(f, "[System] {}", self.content),
        }
    }
}
