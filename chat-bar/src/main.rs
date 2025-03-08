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
    Git,
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
    pub fn wrap_text(text: String, max_width: usize) -> Vec<String> {
        text.lines()
            .flat_map(|line| {
                line.chars()
                    .collect::<Vec<char>>()
                    .chunks(max_width)
                    .map(|chunk| chunk.iter().collect::<String>())
                    .collect::<Vec<String>>()
            })
            .collect()
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
            Git => Line::default().spans(vec![
                Span::styled(
                    format!("{}", ""),
                    Style::default()
                        .fg(gen_color_by_hash(&m.from))
                        .add_modifier(Modifier::ITALIC),
                ),
                m.content[0].clone().into(),
            ]),
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
            MsgKind::Git => write!(f, "[Git] {}:{}", self.from, self.content[0]),
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

pub fn get_repo() -> color_eyre::Result<Repository> {
    Ok(Repository::discover(".")?)
}

fn split_strings_in_vec(vec: Vec<String>, delimiter: char) -> Vec<Vec<String>> {
    vec.into_iter()
        .map(|s| s.split(delimiter).map(|s| s.to_string()).collect())
        .collect()
}

fn split_into_chunks(vec: Vec<String>, chunk_size: usize) -> Vec<Vec<String>> {
    vec.chunks(chunk_size).map(|chunk| chunk.to_vec()).collect()
}

fn main() -> color_eyre::Result<()> {
    //App
    let mut terminal = init_terminal()?;
    //let app = App::default().run(&mut terminal)?;
    let mut app = App::default();

    //repo
    let repo = get_repo()?;

    // Get the reference to HEAD
    let head = repo.head()?;

    // Print the name of HEAD (e.g., "refs/heads/main" or "HEAD")
    //println!("HEAD: {}", head.name().unwrap_or("HEAD"));

    // Get the commit object that HEAD points to
    let commit = head.peel_to_commit()?;

    // Print the commit ID (SHA-1 hash)
    //println!("Commit ID: {}", commit.id());
    //println!("Commit Summary: {:?}", commit.summary());

    //// Optionally, print other commit information
    //println!(
    //    "Commit message: {}",
    //    commit.message().unwrap_or("No message")
    //);

    let mut char_vec: Vec<char> = Vec::new();
    for line in commit.summary().unwrap_or("HEAD").chars() {
        char_vec.push(line);
    }
    char_vec.push(' ');
    let commit_summary = collect_chars_to_string(&char_vec);
    //println!("commit_summary:\n\n{}\n\n", commit_summary);
    let mut commit_message: Vec<String> = Vec::new();
    //commit_message.push(String::from(""));

    for line in commit.body() {
        commit_message.push(String::from(line));
    }
    //let commit_message = collect_chars_to_string(&char_vec);
    //println!("commit_message:\n\n{:?}\n\n", commit_message);

    //let chunks = split_into_chunks(commit_message, 2);
    // println!("{:?}", chunks); // Output: [["a

    let commit_message = split_strings_in_vec(commit_message.clone(), '\n');
    //println!("{:?}", commit_message);

    //std::process::exit(0);

    //env
    let args_vec: Vec<String> = env_args().collect();
    trace!("Arguments:");
    for (index, arg) in args_vec.iter().enumerate() {
        if Some(index) == Some(0) {
            trace!("Some(index) = Some(0):  {}: {}", index, arg);
        } else {
            trace!("  {}: {}", index, arg);
        }
    }

    let cli_args = Args::parse();
    for _ in 0..cli_args.count {
        println!("Hello {}!", cli_args.name);
    }

    debug!("cli_args.log_level {}!", cli_args.log_level.clone());
    if cli_args.log_level.len() > 0 {
        debug!("log_level {}!", cli_args.log_level.clone());

        Builder::from_env(
            Env::default().default_filter_or(
                cli_args.log_level.clone() + ",libp2p_gossipsub::behaviour=error",
            ),
        )
        .init();
    } else {
        Builder::from_env(
            Env::default().default_filter_or("none,libp2p_gossipsub::behaviour=error"),
        )
        .init();
    }

    let (peer_tx, mut peer_rx) = tokio::sync::mpsc::channel::<Msg>(100);
    let (input_tx, input_rx) = tokio::sync::mpsc::channel::<Msg>(100);

    // let input_loop_fut = input_loop(input_tx);
    let input_tx_clone = input_tx.clone();
    app.on_submit(move |m| {
        debug!("sent: {:?}", m);
        input_tx_clone.blocking_send(m).unwrap();
    });

    //topic
    //println!("cli_args.topic {}!", cli_args.topic);
    let mut topic;
    if cli_args.topic.len() > 0 {
        topic = String::from(format!("{}", cli_args.topic.clone()));
    } else {
        //topic = String::from(format!("{:0>64}", 0));
        for line in String::from_utf8_lossy(commit.message_bytes()).lines() {
            app.add_message(
                Msg::default()
                    //.set_content(format!("{:?}", line))
                    .set_content(format!("{:}", line))
                    .set_kind(MsgKind::Git),
            );
        }
        topic = String::from(format!("TOPIC> {} {}", commit.id(), commit_summary));
        app.add_message(
            Msg::default()
                .set_content(topic.clone())
                .set_kind(MsgKind::Chat),
        );
    }

    //app.add_message(
    //    Msg::default()
    //        .set_content(topic.clone())
    //        .set_kind(MsgKind::Command),
    //);

    //debug!("{}", topic);
    let topic = gossipsub::IdentTopic::new(format!("{}", topic));
    //debug!("{}", topic);
    global_rt().spawn(async move {
        evt_loop(input_rx, peer_tx, topic).await.unwrap();
    });
    //topic

    // recv from peer
    let mut tui_msg_adder = app.add_msg_fn();
    global_rt().spawn(async move {
        while let Some(m) = peer_rx.recv().await {
            debug!("recv: {:?}", m);
            tui_msg_adder(m);
        }
    });
    // say hi
    let input_tx_clone = input_tx.clone();
    global_rt().spawn(async move {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        input_tx_clone
            .send(Msg::default().set_kind(MsgKind::Join))
            .await
            .unwrap();
    });

    //app.run
    app.run(&mut terminal)?;

    // say goodbye
    input_tx.blocking_send(Msg::default().set_kind(MsgKind::Leave))?;
    std::thread::sleep(Duration::from_millis(500));

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    restore_terminal()?;
    Ok(())
}

//global_rt
fn global_rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceCell<tokio::runtime::Runtime> = OnceCell::new();
    RT.get_or_init(|| tokio::runtime::Runtime::new().unwrap())
}

//this formats and prints the commit header/message
fn print_commit_header(commit: &Commit) {
    println!("commit {}", commit.id());

    if commit.parents().len() > 1 {
        print!("Merge:");
        for id in commit.parent_ids() {
            print!(" {:.8}", id);
        }
        println!();
    }

    let author = commit.author();
    println!("Author: {}", author);
    print_time(&author.when(), "Date:   ");
    println!();

    for line in String::from_utf8_lossy(commit.message_bytes()).lines() {
        println!("    {}", line);
    }
    println!();
}

//called from above
//part of formatting the output
fn print_time(time: &Time, prefix: &str) {
    let (offset, sign) = match time.offset_minutes() {
        n if n < 0 => (-n, '-'),
        n => (n, '+'),
    };
    let (hours, minutes) = (offset / 60, offset % 60);
    let ts = time::Timespec::new(time.seconds() + (time.offset_minutes() as i64) * 60, 0);
    let time = time::at(ts);

    println!(
        "{}{} {}{:02}{:02}",
        prefix,
        time.strftime("%a %b %e %T %Y").unwrap(),
        sign,
        hours,
        minutes
    );
}

fn collect_chars_to_string(chars: &[char]) -> String {
    chars.iter().collect()
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

fn init_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout()))
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen,)
}

/// App holds the state of the application
pub struct App {
    header_content: String,
    /// Current value of the input box
    input: Input,
    /// Current input mode
    input_mode: InputMode,
    /// History of recorded messages
    messages: Arc<Mutex<Vec<Msg>>>,
    menu: MenuState<Action>,
    _on_input_enter: Option<Box<dyn FnMut(Msg)>>,
    msgs_scroll: usize,
}

impl Default for App {
    fn default() -> Self {
        App {
            header_content: String::new(),
            input: Input::default(),
            input_mode: InputMode::default(),
            messages: Default::default(),
            _on_input_enter: None,
            msgs_scroll: usize::MAX,
            menu: MenuState::new(vec![
                MenuItem::item("gnostr>", Action::Home),
                MenuItem::group(
                    "File",
                    vec![
                        MenuItem::item("New", Action::FileNew),
                        MenuItem::item("Open", Action::FileOpen),
                        MenuItem::group(
                            "Open recent",
                            ["file_1.txt\nline 1\nline 2", "file_2.txt"]
                                .iter()
                                .map(|&f| MenuItem::item(f, Action::FileOpenRecent(f.into())))
                                .collect(),
                        ),
                        MenuItem::item("Save as", Action::FileSaveAs),
                        MenuItem::item("Exit", Action::Exit),
                    ],
                ),
                MenuItem::group(
                    "Edit",
                    vec![
                        MenuItem::item("Copy", Action::EditCopy),
                        MenuItem::item("Cut", Action::EditCut),
                        MenuItem::item("Paste", Action::EditPaste),
                    ],
                ),
                MenuItem::group(
                    "About",
                    vec![
                        MenuItem::item("Author", Action::AboutAuthor),
                        MenuItem::item("Help", Action::AboutHelp),
                    ],
                ),
            ]),
        }
    }
}

impl App {
    pub fn on_submit<F: FnMut(Msg) + 'static>(&mut self, hook: F) {
        self._on_input_enter = Some(Box::new(hook));
    }

    pub fn add_message(&self, msg: Msg) {
        let mut msgs = self.messages.lock().unwrap();
        Self::add_msg(&mut msgs, msg);
    }

    fn add_msg(msgs: &mut Vec<Msg>, msg: Msg) {
        msgs.push(msg);
    }

    pub fn add_msg_fn(&self) -> Box<dyn FnMut(Msg) + 'static + Send> {
        let m = self.messages.clone();
        Box::new(move |msg| {
            let mut msgs = m.lock().unwrap();
            Self::add_msg(&mut msgs, msg);
        })
    }

    //pub fn run(&mut self) -> Result<(), Box<dyn Error>> {
    //    // setup terminal
    //    enable_raw_mode()?;
    //    let mut stdout = io::stdout();
    //    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    //    let backend = CrosstermBackend::new(stdout);
    //    let mut terminal = Terminal::new(backend)?;

    //    // run app
    //    run_app(&mut terminal, self)?;

    //    // restore terminal
    //    disable_raw_mode()?;
    //    execute!(
    //        terminal.backend_mut(),
    //        LeaveAlternateScreen,
    //        DisableMouseCapture
    //    )?;
    //    terminal.show_cursor()?;

    //    Ok(())
    //}
}

#[derive(Debug, Clone)]
enum Action {
    Home,
    FileNew,
    FileOpen,
    FileOpenRecent(String),
    FileSaveAs,
    Exit,
    EditCopy,
    EditCut,
    EditPaste,
    AboutAuthor,
    AboutHelp,
}

impl App {
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
                        Action::Home => {
                            self.header_content = format!("Welcome to Gnostr Chat");
                        }
                        Action::Exit => {
                            return Ok(());
                        }
                        Action::FileNew => {
                            self.header_content.clear();
                        }
                        Action::FileOpenRecent(file) => {
                            self.header_content = format!("content of {file}");
                        }
                        action => {
                            self.header_content = format!("{action:?} not implemented");
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
                                        .set_content(String::from("else:command prompt testing..."));
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
                                        .set_content(String::from("else>command prompt testing..."));
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
                                let l = self.messages.lock().unwrap().len();
                                self.msgs_scroll = self.msgs_scroll.saturating_sub(1).min(l);
                            }
                            KeyCode::Down => {
                                let l = self.messages.lock().unwrap().len();
                                self.msgs_scroll = self.msgs_scroll.saturating_add(1).min(l);
                            }
                            _ => {}
                        },
                    }
                }
            }
        }
    }
    pub fn test_function() -> () {}

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

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        //let width = chunks[1].width.max(3) - 3; // keep 2 for borders and 1 for cursor
        //let scroll = self.input.visual_scroll(width as usize);

        //let vertical = Layout::vertical([Length(3), Min(1), Length(3)]);
        //let [title_area, main_area, status_area] = vertical.areas(area);
        //let horizontal = Layout::horizontal([Fill(1); 2]);
        //let [left_area, right_area] = horizontal.areas(main_area);

        //let widget_blocks = Widget::render(
        //Block::bordered().title("Title Bar"), title_area);
        //Block::bordered().title("Status Bar"), status_area);
        //Block::bordered().title("Left"), left_area);
        //Block::bordered().title("Right"), right_area);
        //);

        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            //.margin(2)
            .constraints(
                [
                    Constraint::Length(1), //0 // MENU
                    Constraint::Length(0), //1 // HEADER
                    Constraint::Fill(1),   //2 // MESSAGE_LIST
                    Constraint::Length(3), //3 // INPUT
                ]
                .as_ref(),
            )
            .split(area);

        let menu_area = vertical_chunks[0];
        let header_area = vertical_chunks[1];
        let message_area = vertical_chunks[2];
        let input_area = vertical_chunks[3];
        let horizontal = Layout::horizontal([Fill(0); 2]).vertical_margin(1); //messages | commit
        let [left_area, right_area] = horizontal.areas(message_area);

        let width = vertical_chunks[0].width.max(3) - 3; // keep 2 for borders and 1 for cursor
        let scroll = self.input.visual_scroll(width as usize);

        //HEADER
        let header = Paragraph::new(self.header_content.as_str())
            .style(match self.input_mode {
                InputMode::Normal => Style::default(),
                _ => Style::default(), //InputMode::Editing => Style::default().fg(Color::Cyan),
                                       //InputMode::Command => Style::default().fg(Color::Yellow),
            })
            .scroll((0, scroll as u16))
            .block(Block::default().borders(Borders::ALL).title("HEADER")) //;
            .render(right_area, buf);

        let height = message_area.height - 0;
        let msgs = self.messages.lock().unwrap();
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
                        .borders(Borders::ALL)
                        .padding(Padding::horizontal(1))
                        .title("messages_vec"),
                )
                .style(match self.input_mode {
                    InputMode::Normal => Style::default(),
                    _ => Style::default(), //InputMode::Editing => Style::default().fg(Color::Cyan),
                                           //InputMode::Command => Style::default().fg(Color::Yellow),
                }),
            left_area,
            buf,
        );

        //let height = main_area.height;
        //let msgs = self.messages.lock().unwrap();
        //let messages_vec: Vec<ListItem> = msgs[0..self.msgs_scroll.min(msgs.len())]
        //    .iter()
        //    .rev()
        //    .map(|m| ListItem::new(Line::from(m)))
        //    .take(height as usize)
        //    .collect();
        //let messages = Widget::render(
        //    List::new(messages_vec)
        //        .direction(ratatui::widgets::ListDirection::BottomToTop)
        //        .block(
        //            Block::default()
        //                .borders(Borders::TOP)
        //                .padding(Padding::horizontal(3))
        //                .title("messages_vec"),
        //        )
        //        .style(match self.input_mode {
        //            InputMode::Normal => Style::default(),
        //            _ => Style::default(), //InputMode::Editing => Style::default().fg(Color::Cyan),
        //                                   //InputMode::Command => Style::default().fg(Color::Yellow),
        //        }),
        //    right_area,
        //    buf,
        //);

        let input = Paragraph::new(self.input.value())
            .style(match self.input_mode {
                InputMode::Normal => Style::default(),
                InputMode::Editing => Style::default().fg(Color::Cyan),
                InputMode::Command => Style::default().fg(Color::Yellow),
            })
            //.scroll((0, scroll as u16))
            .block(Block::default().borders(Borders::ALL).title("Input2"))
            //.wrap(Wrap { trim: true })
            .render(input_area, buf);

        // draw menu last, so it renders on top of other content
        Menu::new().render(menu_area, buf, &mut self.menu);
    }
}

const TOPIC: &str = "chat-bar";

// We create a custom network behaviour that combines Gossipsub and Mdns.
#[derive(NetworkBehaviour)]
pub struct MyBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
}

/// mempool_url
pub async fn async_prompt(mempool_url: String) -> String {
    let s = tokio::spawn(async move {
        let agent: Agent = ureq::AgentBuilder::new()
            .timeout_read(Duration::from_secs(10))
            .timeout_write(Duration::from_secs(10))
            .build();
        let body: String = agent
            .get(&mempool_url)
            .call()
            .expect("")
            .into_string()
            .expect("mempool_url:body:into_string:fail!");

        body
    });

    s.await.unwrap()
}

async fn fetch_data_async(url: String) -> Result<ureq::Response, ureq::Error> {
    task::spawn_blocking(move || {
        let response = ureq::get(&url).call();
        response
    })
    .await
    .unwrap() // Handle potential join errors
}

pub async fn evt_loop(
    mut send: tokio::sync::mpsc::Receiver<Msg>,
    recv: tokio::sync::mpsc::Sender<Msg>,
    topic: gossipsub::IdentTopic,
) -> Result<(), Box<dyn Error>> {
    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_behaviour(|key| {
            // NOTE: To content-address message,
            // we can take the hash of message
            // and use it as an ID.
            // This is used to deduplicate messages.
            //
            // let message_id_fn = |message: &gossipsub::Message| {
            //     let mut s = DefaultHasher::new();
            //     message.data.hash(&mut s);
            //     gossipsub::MessageId::from(s.finish().to_string())
            // };

            // Set a custom gossipsub configuration
            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(10))
                // This is set to aid debugging by not cluttering the log space
                .validation_mode(gossipsub::ValidationMode::Strict)
                // This sets the kind of message validation.
                // The default is Strict (enforce message signing)
                // .message_id_fn(message_id_fn)
                // content-address messages.
                // No two messages of the same content will be propagated.
                .build()
                .map_err(|msg| io::Error::new(io::ErrorKind::Other, msg))?;
            // Temporary hack because `build` does not return a proper `std::error::Error`.

            // build a gossipsub network behaviour
            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )?;

            let mdns = libp2p::mdns::tokio::Behaviour::new(
                libp2p::mdns::Config::default(),
                key.public().to_peer_id(),
            )?;
            Ok(MyBehaviour { gossipsub, mdns })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    // subscribes to our topic
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

    // Listen on all interfaces and whatever port the OS assigns
    swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    debug!("Enter messages via STDIN and they will be sent to connected peers using Gossipsub");

    // Kick it off
    loop {
        select! {
            Some(m) = send.recv() => {
                if let Err(e) = swarm
                    .behaviour_mut().gossipsub
                    .publish(topic.clone(), serde_json::to_vec(&m)?) {
                    warn!("Publish error: {e:?}");
                    let m = Msg::default().set_content(format!("publish error: {e:?}")).set_kind(MsgKind::System);
                    recv.send(m).await?;
                }
            }
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                    for (peer_id, _multiaddr) in list {
                        debug!("mDNS discovered a new peer: {peer_id}");
                        swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                        // let m = Msg::default().set_content(format!("discovered new peer: {peer_id}")).set_kind(MsgKind::System);
                        // recv.send(m).await?;
                    }
                },
                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                    for (peer_id, _multiaddr) in list {
                        debug!("mDNS discover peer has expired: {peer_id}");
                        swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                        // let m = Msg::default().set_content(format!("peer expired: {peer_id}")).set_kind(MsgKind::System);
                        // recv.send(m).await?;
                    }
                },
                SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                    propagation_source: peer_id,
                    message_id: id,
                    message,
                })) => {
                    debug!(
                        "Got message: '{}' with id: {id} from peer: {peer_id}",
                        String::from_utf8_lossy(&message.data),
                    );
                    match serde_json::from_slice::<Msg>(&message.data) {
                        Ok(msg) => {
                            recv.send(msg).await?;
                        },
                        Err(e) => {
                            warn!("Error deserializing message: {e:?}");
                            let m = Msg::default().set_content(format!("Error deserializing message: {e:?}")).set_kind(MsgKind::System);
                            recv.send(m).await?;
                        }
                    }
                },
                SwarmEvent::NewListenAddr { address, .. } => {
                    debug!("Local node is listening on {address}");
                }
                _ => {}
            }
        }
    }
}
