use futures::stream::StreamExt;
use libp2p::{gossipsub, mdns, noise, swarm::NetworkBehaviour, swarm::SwarmEvent, tcp, yamux};
use ratatui::prelude::Constraint::Fill;
use ratatui::prelude::Constraint::Min;
use ratatui::widgets::Padding;
use std::error::Error;
use tokio::{select, task};
use tracing::warn;
use ureq::Agent;

use git2::Config;
//use crate::gossipsub::Config;
//use libp2p::gossipsub::Behaviour;
//use libp2p::mdns::tokio::Behaviour;
use env_logger::{Builder, Env};
use git2::Commit;
use git2::Oid;
use git2::Repository;
use git2::Time;

use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::{
    env,
    env::args as env_args,
    io::{self, stdout, Stdout},
    sync::{Arc, Mutex},
    time::Duration,
};

use tracing::{debug, trace};

//use clap::parser::ValueSource;
use clap::{Arg, ArgAction, ArgMatches, Command, Parser, Subcommand};

use color_eyre::config::HookBuilder;
use color_eyre::eyre::{Result, WrapErr};
use ratatui::prelude::Constraint::Length;
use ratatui::{
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::{Constraint, Direction, Layout},
    prelude::{Backend, Buffer, CrosstermBackend, Rect, StatefulWidget, Terminal, Widget},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use tui_menu::{Menu, MenuEvent, MenuItem, MenuState};

use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use fancy_example::App;



const TOPIC: &str = "chat-bar";
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
    Command,
    GitCommitHeader,
    GitCommitBody,
    GitCommitTime,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Msg {
    pub from: String,
    pub content: Vec<String>,
    pub kind: MsgKind,
}

impl Default for Msg {
    fn default() -> Self {
        Self {
            from: USER_NAME.clone(),
            content: vec!["".to_string(), "".to_string()],
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
        self.content[0] = content;
        self
    }
    pub fn wrap_text(mut self, text: Msg, max_width: usize) -> Self {
        //	for line in text.content.bytes() {

        //    line
        //        .flat_map(|line| {
        //            line.chars()
        //                .collect::<Vec<char>>()
        //                .chunks(max_width)
        //                .map(|chunk| chunk.iter().collect::<String>())
        //                .collect::<Vec<String>>()
        //        })
        //        .collect()
        //}
        //	//return line

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
            Chat => {
                if m.from == *USER_NAME {
                    Line::default().left_aligned().spans(vec![
                        Span::styled(
                            format!("{}{} ", &m.from, ">"),
                            Style::default().fg(gen_color_by_hash(&m.from)),
                        ),
                        m.content[0].clone().into(),
                    ])
                } else {
                    Line::default().right_aligned().spans(vec![
                        m.content[0].clone().into(),
                        Span::styled(
                            format!(" {}{}", "<", &m.from),
                            Style::default().fg(gen_color_by_hash(&m.from)),
                        ),
                    ])
                }
            }
            Raw => m.content[0].clone().into(),
            Command => Line::default().spans(vec![
                Span::styled(
                    format!("Command: {}{} ", &m.from, ">"),
                    Style::default()
                        .fg(gen_color_by_hash(&m.from))
                        .add_modifier(Modifier::ITALIC),
                ),
                m.content[0].clone().into(),
            ]),
            Git => Line::default().spans(
                vec![
                    Span::styled(
                        format!("{}", m.content[0].clone()),
                        Style::default()
                            .fg(gen_color_by_hash(&m.from))
                            .add_modifier(Modifier::ITALIC),
                    ),
                    //m.content[1].clone().into(),
                ]
                .iter()
                .map(|i| format!("{}", i)),
            ),
            GitCommitHeader => Line::default().spans(
                vec![
                    Span::styled(
                        format!("{}", m.content[0].clone()),
                        Style::default()
                            .fg(gen_color_by_hash(&m.from))
                            .add_modifier(Modifier::ITALIC),
                    ),
                    m.content[1].clone().into(),
                ]
                .iter()
                .map(|i| format!("{}", i)),
            ),
            GitCommitBody => Line::default().spans(
                vec![
                    Span::styled(
                        format!("{}", m.content[0].clone()),
                        Style::default()
                            .fg(gen_color_by_hash(&m.from))
                            .add_modifier(Modifier::ITALIC),
                    ),
                    m.content[1].clone().into(),
                ]
                .iter()
                .map(|i| format!("{}", i)),
            ),
            GitCommitTime => Line::default().spans(
                vec![
                    Span::styled(
                        format!("{}", m.content[0].clone()),
                        Style::default()
                            .fg(gen_color_by_hash(&m.from))
                            .add_modifier(Modifier::ITALIC),
                    ),
                    m.content[1].clone().into(),
                ]
                .iter()
                .map(|i| format!("{}", i)),
            ),
        }
    }
}

impl Display for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            MsgKind::Join => write!(f, "{} join", self.from),
            MsgKind::Leave => write!(f, "{} left", self.from),
            MsgKind::Chat => write!(f, "{}: {}", self.from, self.content[0]),
            MsgKind::System => write!(f, "[System] {}", self.content[0]),
            MsgKind::Raw => write!(f, "{}", self.content[0]),
            MsgKind::Command => write!(f, "[Command] {}:{}", self.from, self.content[0]),
            MsgKind::GitCommitHeader => {
                write!(f, "[GitCommitHeader] {}:{}", self.from, self.content[0])
            }
            MsgKind::GitCommitBody => {
                write!(f, "[GitCommitBody] {}:{}", self.from, self.content[0])
            }
            MsgKind::GitCommitTime => {
                write!(f, "[GitCommitTime] {}:{}", self.from, self.content[0])
            }
        }
    }
}

#[derive(Default)]
enum InputMode {
    #[default]
    Normal,
    //#[default]
    Editing,
    Command,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Name of the person to greet
    #[arg(short, long, default_value = "user")]
    name: String,

    /// Number of times to greet
    #[arg(short, long, default_value_t = 1)]
    count: u8,
    #[arg(short = 't', long)]
    tui: bool,
    #[arg(long = "cfg", default_value = "")]
    config: String,
    #[arg(long = "log_level", default_value = "")]
    log_level: String,
    #[arg(long = "topic", default_value = "")]
    topic: String,
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    use eframe::NativeOptions;
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Unable to create Runtime");

    // Enter the runtime so that `tokio::spawn` is available immediately.
    let _enter = rt.enter();

    // Execute the runtime in its own thread.
    // The future doesn't have to do anything. In this example, it just sleeps forever.
    std::thread::spawn(move || {
        rt.block_on(async {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });
    });

    eframe::run_native(
        "Dnd Example App",
        NativeOptions::default(),
        Box::new(move |ctx| Ok(Box::new(App::new(&ctx.egui_ctx)) as Box<dyn eframe::App>)),
    )
}

// when compiling to web using trunk.
#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;
    let web_options = eframe::WebOptions::default();
    let element = eframe::web_sys::window()
        .expect("failed to get window")
        .document()
        .expect("failed to get document")
        .get_element_by_id("canvas")
        .expect("failed to get canvas element")
        .dyn_into::<eframe::web_sys::HtmlCanvasElement>()
        .unwrap();
    wasm_bindgen_futures::spawn_local(async {
        eframe::WebRunner::new()
            .start(
                element,
                web_options,
                Box::new(|ctx| Ok(Box::new(App::new(&ctx.egui_ctx)) as Box<dyn eframe::App>)),
            )
            .await
            .expect("failed to start eframe");
    });
}
