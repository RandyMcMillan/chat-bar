//use futures::stream::StreamExt;
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

use eframe::egui::Color32;
use eframe::emath::lerp;
use eframe::{egui, Frame};
use egui::{Context, Id, SidePanel, Ui};
use std::hash::Hash;
use std::num::NonZeroUsize;

use egui_inbox::UiInbox;
use egui_router::EguiRouter;
use hello_egui_utils::center::Center;
use shared_state::SharedState;
use sidebar::SideBar;

use crate::routes::router;

pub mod chat;
mod color_sort;
mod crate_ui;
pub mod example;
mod flex;
mod futures;
pub mod gallery;
mod routes;
mod shared_state;
mod sidebar;
mod signup_form;
pub mod stargazers;

pub enum FancyMessage {
    Navigate(String),
}

pub struct App {
    sidebar_expanded: bool,
    shared_state: SharedState,
    inbox: UiInbox<FancyMessage>,
    router: EguiRouter<SharedState>,
}

impl App {
    pub fn new(ctx: &Context) -> Self {
        let (tx, inbox) = UiInbox::channel();
        let mut state = SharedState::new(tx);

        let router = router(&mut state);

        ctx.options_mut(|opts| {
            opts.max_passes = NonZeroUsize::new(4).unwrap();
        });

        egui_extras::install_image_loaders(ctx);
        egui_thumbhash::register(ctx);

        Self {
            inbox,
            shared_state: state,
            sidebar_expanded: false,
            router,
        }
    }

    pub fn show(&mut self, ctx: &Context) {
        self.inbox.set_ctx(ctx);
        self.inbox.read_without_ctx().for_each(|msg| match msg {
            FancyMessage::Navigate(route) => {
                self.router.navigate(&mut self.shared_state, route).unwrap();
            }
        });

        let width = ctx.screen_rect().width();
        let collapsible_sidebar = width < 800.0;
        let is_expanded = !collapsible_sidebar || self.sidebar_expanded;

        SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(170.0)
            .show_animated(ctx, is_expanded, |ui| {
                if SideBar::ui(ui, &mut self.shared_state) {
                    self.sidebar_expanded = false;
                }
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(ctx.style().visuals.panel_fill.gamma_multiply(0.7)))
            .show(ctx, |ui| {
                vertex_gradient(
                    ui,
                    &Gradient(
                        self.shared_state
                            .background_colors
                            .iter()
                            .map(|c| c.color)
                            .collect(),
                    ),
                );

                if collapsible_sidebar {
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        if ui.add(egui::Button::new("☰")).clicked() {
                            self.sidebar_expanded = !self.sidebar_expanded;
                        }
                    });
                }

                if !(collapsible_sidebar && is_expanded) {
                    self.router.ui(ui, &mut self.shared_state);
                }
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        self.show(ctx);
    }
}

pub fn demo_area(ui: &mut Ui, title: &'static str, width: f32, content: impl FnOnce(&mut Ui)) {
    Center::new(title).ui(ui, |ui| {
        let width = f32::min(ui.available_width() - 20.0, width);
        ui.set_max_width(width);
        ui.set_max_height(ui.available_height() - 20.0);

        egui::Frame::NONE
            .fill(ui.style().visuals.panel_fill)
            .corner_radius(4.0)
            .inner_margin(20.0)
            .show(ui, |ui| {
                ui.heading(title);
                ui.add_space(5.0);

                content(ui);
            });
    });
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Gradient(pub Vec<Color32>);

// taken from the egui demo
fn vertex_gradient(ui: &mut Ui, gradient: &Gradient) {
    use egui::epaint::{pos2, Mesh, Shape};

    let rect = ui.max_rect();

    let n = gradient.0.len();
    let animation_time = 0.4;
    assert!(n >= 2);
    let mut mesh = Mesh::default();
    for (i, &color) in gradient.0.iter().enumerate() {
        let t = i as f32 / (n as f32 - 1.0);
        let y = lerp(rect.y_range(), t);
        mesh.colored_vertex(
            pos2(rect.left(), y),
            animate_color(ui, color, Id::new("a").with(i), animation_time),
        );
        mesh.colored_vertex(
            pos2(rect.right(), y),
            animate_color(ui, color, Id::new("b").with(i), animation_time),
        );
        if i < n - 1 {
            let i = i as u32;
            mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
            mesh.add_triangle(2 * i + 1, 2 * i + 2, 2 * i + 3);
        }
    }
    ui.painter().add(Shape::mesh(mesh));
}

fn animate_color(ui: &mut Ui, color: Color32, id: Id, duration: f32) -> Color32 {
    Color32::from_rgba_premultiplied(
        ui.ctx()
            .animate_value_with_time(id.with(0), f32::from(color[0]), duration) as u8,
        ui.ctx()
            .animate_value_with_time(id.with(1), f32::from(color[1]), duration) as u8,
        ui.ctx()
            .animate_value_with_time(id.with(2), f32::from(color[2]), duration) as u8,
        color[3],
    )
}

const TOPIC: &str = "chat-bar";
pub(crate) static USER_NAME: Lazy<String> = Lazy::new(|| {
    format!(
        "{}",
        std::env::var("USER")
            .unwrap_or_else(|_| hostname::get().unwrap().to_string_lossy().to_string()),
    )
});

#[derive(Default)]
enum InputMode {
    #[default]
    Normal,
    //#[default]
    Editing,
    Command,
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

#[derive(Debug, Clone)]
enum MenuAction {
    Home,
    FileNew,
    FileOpen(String),
    FileOpenRecent(String),
    FileSaveAs,
    Exit,
    EditCopy,
    EditCut,
    EditPaste,
    AboutAuthor,
    AboutHelp,
}

/// App holds the state of the application
pub struct TuiApp {
    topic: String,
    header_content: String,
    /// Current value of the input box
    input: tui_input::Input,
    /// Current input mode
    input_mode: InputMode,
    /// History of recorded messages
    messages: Arc<Mutex<Vec<Msg>>>,
    commit_messages: Arc<Mutex<Vec<Msg>>>,
    menu: MenuState<MenuAction>,
    _on_input_enter: Option<Box<dyn FnMut(Msg)>>,
    msgs_scroll: usize,
    commit_msgs_scroll: usize,
}

impl Default for TuiApp {
    fn default() -> Self {
        TuiApp {
            topic: String::from("..............."),
            header_content: String::new(),
            input: Input::default(),
            input_mode: InputMode::default(),
            messages: Default::default(),
            commit_messages: Default::default(),
            _on_input_enter: None,
            msgs_scroll: 0 as usize,
            commit_msgs_scroll: 14 as usize, // change with layout
            menu: MenuState::new(vec![
                MenuItem::item("gnostr>", MenuAction::Home),
                MenuItem::group(
                    "File",
                    vec![
                        MenuItem::item("New", MenuAction::FileNew),
                        MenuItem::group(
                            "Open",
                            ["file_1.txt\nline 1\nline 2", "file_2.txt"]
                                .iter()
                                .map(|&f| MenuItem::item(f, MenuAction::FileOpen(f.into())))
                                .collect(),
                        ),
                        MenuItem::group(
                            "Open recent",
                            ["file_1.txt\nline 1\nline 2", "file_2.txt"]
                                .iter()
                                .map(|&f| MenuItem::item(f, MenuAction::FileOpenRecent(f.into())))
                                .collect(),
                        ),
                        MenuItem::item("Save as", MenuAction::FileSaveAs),
                        MenuItem::item("Exit", MenuAction::Exit),
                    ],
                ),
                MenuItem::group(
                    "Edit",
                    vec![
                        MenuItem::item("Copy", MenuAction::EditCopy),
                        MenuItem::item("Cut", MenuAction::EditCut),
                        MenuItem::item("Paste", MenuAction::EditPaste),
                    ],
                ),
                MenuItem::group(
                    "About",
                    vec![
                        MenuItem::item("Author", MenuAction::AboutAuthor),
                        MenuItem::item("Help", MenuAction::AboutHelp),
                    ],
                ),
            ]),
        }
    }
}

/// impl TuiApp
impl TuiApp {
    pub fn on_submit<F: FnMut(Msg) + 'static>(&mut self, hook: F) {
        self._on_input_enter = Some(Box::new(hook));
    }

    //ADD MESSAGE
    //add_message
    pub fn add_message(&self, msg: Msg) {
        let mut msgs = self.messages.lock().unwrap();
        Self::add_msg(&mut msgs, msg);
    }

    //add_msg
    fn add_msg(msgs: &mut Vec<Msg>, msg: Msg) {
        msgs.push(msg.clone().wrap_text(msg.clone(), 80));
    }

    //add_msg_fn
    pub fn add_msg_fn(&self) -> Box<dyn FnMut(Msg) + 'static + Send> {
        let m = self.messages.clone();
        Box::new(move |msg| {
            let mut msgs = m.lock().unwrap();
            Self::add_msg(&mut msgs, msg);
        })
    }

    //ADD COMMIT MESSAGE
    //add_commit_message
    pub fn add_commit_message(&self, msg: Msg) {
        let mut commit_msgs = self.commit_messages.lock().unwrap();
        Self::add_commit_msg(&mut commit_msgs, msg);
    }

    //add_commit_msg
    fn add_commit_msg(commit_msgs: &mut Vec<Msg>, commit_msg: Msg) {
        commit_msgs.push(commit_msg.clone().wrap_text(commit_msg.clone(), 80));
    }

    //add_commit_msg_fn
    pub fn add_commit_msg_fn(&self) -> Box<dyn FnMut(Msg) + 'static + Send> {
        let m = self.commit_messages.clone();
        Box::new(move |commit_msg| {
            let mut msgs = m.lock().unwrap();
            Self::add_msg(&mut msgs, commit_msg);
        })
    }
}

/// impl Widget for &mut TuiApp
impl Widget for &mut TuiApp {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // LAYOUT
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(0)
            .constraints(
                [
                    Constraint::Length(1), //0 // MENU
                    //TODO HEADER Length(1) if not TOPIC commit
                    Constraint::Length(8), //1 // HEADER
                    //TODO MESSAGE_LIST hide COMMIT_CONTENT if not TOPIC commit
                    Constraint::Fill(100), //2 // MESSAGE_LIST
                    // messages | topic content
                    Constraint::Length(3), //3 // INPUT
                ]
                .as_ref(),
            )
            .split(area);

        let menu_area = vertical_chunks[0]; // MENU
        let header_area = vertical_chunks[1]; // HEADER
                                              //TODO MESSAGE_LIST hide COMMIT_CONTENT if not TOPIC commit
        let message_area = vertical_chunks[2]; // MESSAGE_LIST
        let horizontal =                       // messages | topic content
            Layout::horizontal([Fill(0); 2])
            .vertical_margin(0);
        let [left_area, right_area] = horizontal.areas(message_area);
        let input_area = vertical_chunks[3]; // INPUT

        // HEADER
        let width = vertical_chunks[0].width.max(3) - 3;
        // keep 2 for borders and 1 for cursor
        let scroll = self.input.visual_scroll(width as usize);

        let mut header_content = Paragraph::new(
            String::from("testing>>>") + &self.topic.to_string() + &String::from("<<<testing"),
        )
        .style(match self.input_mode {
            InputMode::Normal => Style::default(),
            //InputMode::Editing => Style::default().fg(Color::Cyan),
            //InputMode::Command => Style::default().fg(Color::Yellow),
            _ => Style::default(),
        })
        .scroll((0, scroll as u16))
        .block(
            Block::default()
                //.padding(Padding::uniform(1))
                //.padding(Padding::horizontal(2))
                //.padding(Padding::left(3))
                //.padding(Padding::proportional(1))
                //.padding(Padding::symmetric(5, 6))
                //left: u16, right: u16, top: u16, bottom: u16
                .padding(Padding::new(1, 1, 0, 0))
                //.padding(Padding::vertical(1))
                .borders(Borders::TOP)
                .title(format!(" TOPIC> {}{}", self.topic.clone(), " ")),
        )
        .render(header_area, buf);

        // TOPIC_CONTENT
        //let width = vertical_chunks[0].width.max(3) - 3;
        //// keep 2 for borders and 1 for cursor
        //let scroll = self.input.visual_scroll(width as usize);

        //let mut topic_content = Paragraph::new(self.header_content.as_str())
        //    .style(match self.input_mode {
        //        InputMode::Normal => Style::default(),
        //        //InputMode::Editing => Style::default().fg(Color::Cyan),
        //        //InputMode::Command => Style::default().fg(Color::Yellow),
        //        _ => Style::default(),
        //    })
        //    .scroll((0, scroll as u16))
        //    .block(
        //        Block::default()
        //            .padding(Padding::new(1, 1, 0, 0))
        //            .borders(Borders::ALL)
        //            .title("TOPIC_CONTENT"),
        //    )
        //    .wrap(Wrap { trim: true })
        //    .render(right_area, buf);

        let height = message_area.height - 0;
        let commit_msgs = self.commit_messages.lock().unwrap();
        let new_msg_list = commit_msgs.clone();

        //for message
        for message in new_msg_list {
            //println!("{}", Line::from(message.to_string()));
        }

        let commit_messages_vec: Vec<ListItem> = commit_msgs
            [0..self.commit_msgs_scroll.min(commit_msgs.len())]
            .iter()
            .rev()
            .map(|m| ListItem::new(Line::from(m)))
            .take(height as usize)
            .collect();
        let commit_messages = Widget::render(
            List::new(commit_messages_vec)
                .direction(ratatui::widgets::ListDirection::BottomToTop)
                .block(
                    Block::default()
                        .borders(Borders::TOP | Borders::LEFT)
                        .padding(Padding::new(1, 1, 0, 0))
                        //.title(self.topic.clone()),
                        .title(" COMMIT_CONTENT "),
                )
                .style(match self.input_mode {
                    InputMode::Normal => Style::default(),
                    //InputMode::Editing => Style::default().fg(Color::Cyan),
                    InputMode::Command => Style::default().fg(Color::Yellow),
                    _ => Style::default(),
                }),
            // TODO MESSAGE_LIST hide COMMIT_CONTENT if not TOPIC commit
            right_area,
            buf,
        );

        // MESSAGES
        let height = message_area.height - 0;
        let msgs = self.messages.lock().unwrap();
        let new_msg_list = msgs.clone();

        //for message
        for message in new_msg_list {
            //println!("{}", Line::from(message.to_string()));
        }

        let messages_vec: Vec<ListItem> = msgs[0..self.msgs_scroll.min(msgs.len())]
            .iter()
            .rev()
            .map(|m| ListItem::new(Line::from(m)))
            .take(height as usize)
            .collect();
        let messages = Widget::render(
            List::new(messages_vec)
                .direction(ratatui::widgets::ListDirection::BottomToTop)
                .block(
                    Block::default()
                        .borders(Borders::TOP | Borders::RIGHT)
                        .padding(Padding::new(1, 1, 0, 0))
                        //.title(self.topic.clone()),
                        .title(" CHAT "),
                )
                .style(match self.input_mode {
                    InputMode::Normal => Style::default(),
                    InputMode::Editing => Style::default().fg(Color::Cyan),
                    //InputMode::Command => Style::default().fg(Color::Yellow),
                    _ => Style::default(),
                }),
            left_area,
            buf,
        );

        // INPUT
        let input = Paragraph::new(self.input.value())
            .style(match self.input_mode {
                InputMode::Normal => Style::default(),
                InputMode::Editing => Style::default().fg(Color::Cyan),
                InputMode::Command => Style::default().fg(Color::Yellow),
            })
            //.scroll((0, scroll as u16))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Input2")
                    .padding(Padding::new(1, 1, 0, 0)),
            )
            .wrap(Wrap { trim: true }) //trim leadingg white space
            .render(input_area, buf);

        // MENU
        // draw menu last, so it renders on top of other content
        Menu::new().render(menu_area, buf, &mut self.menu);
    }
}

/// Install panic and error hooks that restore the terminal before printing the error.
pub fn init_hooks() -> color_eyre::Result<()> {
    let (panic, error) = HookBuilder::default().into_hooks();
    let panic = panic.into_panic_hook();
    let error = error.into_eyre_hook();

    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal(); // ignore failure to restore terminal
        panic(info);
    }));
    color_eyre::eyre::set_hook(Box::new(move |e| {
        let _ = restore_terminal(); // ignore failure to restore terminal
        error(e)
    }))?;

    Ok(())
}

pub fn init_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout()))
}

pub fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen,)
}

/// impl TuiApp::run
impl TuiApp {
    fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // run app
        //run_app(&mut terminal, self)?;

        let tick_rate = Duration::from_millis(10);
        loop {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.size()))?;

            for e in self.menu.drain_events() {
                match e {
                    MenuEvent::Selected(item) => match item {
                        MenuAction::Home => {
                            //code block
                            self.header_content = format!("Welcome to Gnostr Chat");
                        }
                        MenuAction::Exit => {
                            return Ok(());
                        }
                        MenuAction::FileNew => {
                            self.header_content.clear();
                        }
                        MenuAction::FileOpenRecent(file) => {
                            self.header_content = format!("content of {file}");
                        }
                        MenuAction::FileOpen(file) => {
                            self.header_content =
                                format!("MenuAction::FileOpen codeblock {{}} {file}");
                        }
                        action => {
                            self.header_content = format!("ACTION {action:?} not implemented");
                        }
                    },
                } // match e end
                self.menu.reset();
            } // for e end

            if event::poll(tick_rate)? {
                if let Event::Key(key) = event::read()? {
                    self.on_key_event(key);

                    match self.input_mode {
                        //command prompts
                        InputMode::Normal => match key.code {
                            //: mode
                            KeyCode::Char(':') => {
                                //self.input.reset(); //TODO
                                self.msgs_scroll = self.messages.lock().unwrap().len();
                                if !self.input.value().trim().is_empty() { //TODO
                                    let m = Msg::default()
                                        .set_content(String::from(":command prompt testing..."));
                                    self.add_message(m.clone());
                                    if let Some(ref mut hook) = self._on_input_enter {
                                        hook(m);
                                    }
                                } else {
                                    let m = Msg::default()
                                        .set_content(String::from("LINE COMMENT:\ncommand prompt testing..."));
                                    self.add_message(m.clone());
                                    if let Some(ref mut hook) = self._on_input_enter {
                                        hook(m);
                                    }

                                }
                                //self.input.handle_event(&Event::Key(key));
                                self.input_mode = InputMode::Command;
                            }
                            KeyCode::Char('>') => {
                                //> mode
                                //self.input.reset(); //TODO
                                self.msgs_scroll = self.messages.lock().unwrap().len();
                                if !self.input.value().trim().is_empty() { //TODO
                                    let m = Msg::default()
                                        .set_content(String::from(">command prompt testing..."));
                                    self.add_message(m.clone());
                                    if let Some(ref mut hook) = self._on_input_enter {
                                        hook(m);
                                    }
                                } else {
                                    let m = Msg::default()
                                        .set_content(String::from("LINE COMMENT REPLY>\ncommand prompt testing..."));
                                    self.add_message(m.clone());
                                    if let Some(ref mut hook) = self._on_input_enter {
                                        hook(m);
                                    }

                             }
                                //self.input.handle_event(&Event::Key(key));
                                self.input_mode = InputMode::Command;
                            }
                            KeyCode::Char('e') | KeyCode::Char('i') => {
                                self.input_mode = InputMode::Editing;
                                self.msgs_scroll = usize::MAX;
                            }
                            KeyCode::Char('q') /*| KeyCode::Esc*/ => {
                                return Ok(());
                            }
                            KeyCode::Up => {
                                let l = self.messages.lock().unwrap().len();
                                self.msgs_scroll = self.msgs_scroll.saturating_sub(1).min(l);
                            }
                            KeyCode::Down => {
                                let l = self.messages.lock().unwrap().len();
                                self.msgs_scroll = self.msgs_scroll.saturating_add(1).min(l);
                            }
                            KeyCode::Enter => {

                                self.msgs_scroll = usize::MAX;

                            }
                            KeyCode::Esc => {

                                self.msgs_scroll = usize::MAX;
                                self.msgs_scroll = usize::MAX;
                                self.input.reset();

				    	    }
                            _ => {
                                //TODO command prompts
                                //eval exec
                                //self.input.handle_event(&Event::Key(key));
                                self.msgs_scroll = usize::MAX;
                            }
                        },
                        InputMode::Editing => match key.code {
                            KeyCode::Enter => {
                                if !self.input.value().trim().is_empty() {
                                    let m =
                                        Msg::default().set_content(self.input.value().to_owned());
                                    self.add_message(m.clone());
                                    if let Some(ref mut hook) = self._on_input_enter {
                                        hook(m);
                                    }
                                }
                                self.input.reset();
                            }
                            KeyCode::Esc => {
                                self.input_mode = InputMode::Normal;
                                self.msgs_scroll = self.messages.lock().unwrap().len();
                                self.msgs_scroll = usize::MAX;
                            }
                            _ => {
                                self.input.handle_event(&Event::Key(key));
                            }
                        },
                        InputMode::Command => match key.code {
                            KeyCode::Esc => {
                                self.input_mode = InputMode::Normal;
                                self.msgs_scroll = self.messages.lock().unwrap().len();
                                self.input.reset();
                            }
                            KeyCode::Enter => {}
                            KeyCode::Up => {
                                let l = self.commit_messages.lock().unwrap().len();
                                self.commit_msgs_scroll =
                                    self.commit_msgs_scroll.saturating_sub(1).min(l);
                            }
                            KeyCode::Down => {
                                let l = self.commit_messages.lock().unwrap().len();
                                self.commit_msgs_scroll =
                                    self.commit_msgs_scroll.saturating_add(1).min(l);
                            }
                            _ => {}
                        },
                    }
                }
            }
        }
    }
    /// impl App::test_function
    pub fn test_function() -> () {}

    /// impl App::on_key_event
    fn on_key_event(&mut self, key: event::KeyEvent) {
        //if !self.input_mode {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL) {
            let _ = restore_terminal();
            std::process::exit(0);
        }

        match self.input_mode {
            InputMode::Normal => match key.code {
                KeyCode::Char('h') | KeyCode::Left => self.menu.left(),
                KeyCode::Char('l') | KeyCode::Right => self.menu.right(),
                KeyCode::Char('j') | KeyCode::Down => self.menu.down(),
                KeyCode::Char('k') | KeyCode::Up => self.menu.up(),
                KeyCode::Esc => self.menu.reset(),
                KeyCode::Enter => self.menu.select(),
                _ => {}
            },
            InputMode::Editing => match key.code {
                _ => {}
            },
            InputMode::Command => match key.code {
                _ => {}
            },
        }
    }
}
