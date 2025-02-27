use env_logger::{Builder, Env};
use git2::Commit;
use git2::Repository;
use git2::Time;
use libp2p::gossipsub;
use once_cell::sync::OnceCell;
use std::{
    env,
    env::args,
    error::Error,
    io::{self, stdout, Stdout},
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{io as tokio_io, io::AsyncBufReadExt};
use tracing::{debug, trace};

use chat_bar::msg;
use chat_bar::p2p;
use chat_bar::ui;
use msg::*;
use p2p::evt_loop;

use color_eyre::config::HookBuilder;
use ratatui::{
    crossterm::{
        event::{self, Event, KeyCode},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::{Constraint, Direction, Layout},
    prelude::{Backend, Buffer, CrosstermBackend, Rect, StatefulWidget, Stylize, Terminal, Widget},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use tui_menu::{Menu, MenuEvent, MenuItem, MenuState};

use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

#[derive(Default)]
enum InputMode {
    #[default]
    Normal,
    //#[default]
    Editing,
    Command,
}

fn main() -> color_eyre::Result<()> {
    let args_vec: Vec<String> = env::args().collect();
    trace!("Arguments:");
    for (index, arg) in args_vec.iter().enumerate() {
        if Some(index) == Some(0) {
            trace!("Some(index) = Some(0):  {}: {}", index, arg);
        } else {
            trace!("  {}: {}", index, arg);
        }
    }

    if let Some(log_level) = args().nth(2) {
        Builder::from_env(
            Env::default().default_filter_or(log_level + ",libp2p_gossipsub::behaviour=error"),
        )
        .init();
    } else {
        Builder::from_env(
            Env::default().default_filter_or("none,libp2p_gossipsub::behaviour=error"),
        )
        .init();
    }

    // Create a Gossipsub topic
    // Open the Git repository
    let repo = Repository::discover(".")?; // Opens the repository in the current directory

    // Get the reference to HEAD
    let head = repo.head()?;

    // Print the name of HEAD (e.g., "refs/heads/main" or "HEAD")
    debug!("HEAD: {}", head.name().unwrap_or("HEAD"));

    // Get the commit object that HEAD points to
    let commit = head.peel_to_commit()?;

    // Print the commit ID (SHA-1 hash)
    debug!("Commit ID: {}", commit.id());

    // Optionally, print other commit information
    debug!(
        "Commit message: {}",
        commit.message().unwrap_or("No message")
    );

    let mut char_vec: Vec<char> = Vec::new();
    for line in commit.summary().unwrap_or("HEAD").chars() {
        char_vec.push(line);
    }
    let commit_summary = collect_chars_to_string(&char_vec);
    //debug!("commit_summary:\n\n{}\n\n", commit_summary);

    let mut topic = String::from(format!("{:0>64}", 0));

    let mut terminal = init_terminal()?;
    let app = App::new().run(&mut terminal)?;

    if let Some(topic_arg) = args().nth(1) {
    } else {
    }

    restore_terminal()?;
    Ok(())
}

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
    content: String,
    /// Current value of the input box
    input: Input,
    /// Current input mode
    input_mode: InputMode,
    /// History of recorded messages
    messages: Arc<Mutex<Vec<msg::Msg>>>,
    menu: MenuState<Action>,
    _on_input_enter: Option<Box<dyn FnMut(msg::Msg)>>,
    msgs_scroll: usize,
}

impl App {
    pub fn on_submit<F: FnMut(msg::Msg) + 'static>(&mut self, hook: F) {
        self._on_input_enter = Some(Box::new(hook));
    }

    pub fn add_message(&self, msg: msg::Msg) {
        let mut msgs = self.messages.lock().unwrap();
        Self::add_msg(&mut msgs, msg);
    }

    fn add_msg(msgs: &mut Vec<msg::Msg>, msg: msg::Msg) {
        msgs.push(msg);
    }

    pub fn add_msg_fn(&self) -> Box<dyn FnMut(msg::Msg) + 'static + Send> {
        let m = self.messages.clone();
        Box::new(move |msg| {
            let mut msgs = m.lock().unwrap();
            Self::add_msg(&mut msgs, msg);
        })
    }

    fn new() -> Self {
        Self {
            content: String::new(),
            input: Input::default(),
            input_mode: InputMode::default(),
            messages: Default::default(),
            _on_input_enter: None,
            msgs_scroll: usize::MAX,
            menu: MenuState::new(vec![
                MenuItem::group(
                    "File",
                    vec![
                        MenuItem::item("New", Action::FileNew),
                        MenuItem::item("Open", Action::FileOpen),
                        MenuItem::group(
                            "Open recent",
                            ["file_1.txt", "file_2.txt"]
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

#[derive(Debug, Clone)]
enum Action {
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
        let tick_rate = Duration::from_millis(1);
        loop {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.size()))?;

            if event::poll(tick_rate)? {
                if let Event::Key(key) = event::read()? {
                    self.on_key_event(key);
                }
            }

            for e in self.menu.drain_events() {
                match e {
                    MenuEvent::Selected(item) => match item {
                        Action::Exit => {
                            return Ok(());
                        }
                        Action::FileNew => {
                            self.content.clear();
                        }
                        Action::FileOpenRecent(file) => {
                            self.content = format!("content of {file}");
                        }
                        action => {
                            self.content = format!("{action:?} not implemented");
                        }
                    },
                } // match e end
                self.menu.reset();
            } // for e end

            if event::poll(tick_rate)? {
                if let Event::Key(key) = event::read()? {
                    //      self.on_key_event(key);
                    //}
                    //}
                    match self.input_mode {
                        //command prompts
                        InputMode::Normal => match key.code {
					//: mode




                    KeyCode::Char(':') => {
                        //self.input.reset(); //TODO
                        self.msgs_scroll = self.messages.lock().unwrap().len();
                        if !self.input.value().trim().is_empty() { //TODO
                            let m = msg::Msg::default()
                                .set_content(String::from(":command prompt testing..."));
                            self.add_message(m.clone());
                            if let Some(ref mut hook) = self._on_input_enter {
                                hook(m);
                            }
						} else {
                            let m = msg::Msg::default()
                                .set_content(String::from("else:command prompt testing..."));
                            self.add_message(m.clone());
                            if let Some(ref mut hook) = self._on_input_enter {
                                hook(m);
                            }

						}
                        self.input.handle_event(&Event::Key(key));
                        self.input_mode = InputMode::Command;
                    }
                    KeyCode::Char('>') => {
						//> mode
                        //self.input.reset(); //TODO
                        self.msgs_scroll = self.messages.lock().unwrap().len();
                        if !self.input.value().trim().is_empty() { //TODO
                            let m = msg::Msg::default()
                                .set_content(String::from(">command prompt testing..."));
                            self.add_message(m.clone());
                            if let Some(ref mut hook) = self._on_input_enter {
                                hook(m);
                            }
						} else {
                            let m = msg::Msg::default()
                                .set_content(String::from("else>command prompt testing..."));
                            self.add_message(m.clone());
                            if let Some(ref mut hook) = self._on_input_enter {
                                hook(m);
                            }

						}
                        self.input.handle_event(&Event::Key(key));
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


					}
                    _ => {
                        //TODO command prompts
                        //eval exec
                        //self.input.handle_event(&Event::Key(key));
                    }
                },
                        InputMode::Editing => match key.code {
                            KeyCode::Enter => {
                                if !self.input.value().trim().is_empty() {
                                    let m = msg::Msg::default()
                                        .set_content(self.input.value().to_owned());
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
        if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL) {
            let _ = restore_terminal();
            std::process::exit(0);
        }

        match key.code {
            KeyCode::Char('h') | KeyCode::Left => self.menu.left(),
            KeyCode::Char('l') | KeyCode::Right => self.menu.right(),
            KeyCode::Char('j') | KeyCode::Down => self.menu.down(),
            KeyCode::Char('k') | KeyCode::Up => self.menu.up(),
            KeyCode::Esc => self.menu.reset(),
            KeyCode::Enter => self.menu.select(),
            _ => {}
        }
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            // .margin(2)
            .constraints(
                [
                    Constraint::Length(1), //0
                    Constraint::Fill(1),   //1
                    Constraint::Length(3), //2
                ]
                .as_ref(),
            )
            .split(area);

        let width = chunks[0].width.max(3) - 3; // keep 2 for borders and 1 for cursor
        let scroll = self.input.visual_scroll(width as usize);

        let header = Paragraph::new(self.content.as_str())
            .style(match self.input_mode {
                InputMode::Normal => Style::default(),
                InputMode::Editing => Style::default().fg(Color::Cyan),
                InputMode::Command => Style::default().fg(Color::Yellow),
            })
            .scroll((0, scroll as u16))
            .block(Block::default().borders(Borders::ALL).title("HEADER"))//;
            .render(chunks[1], buf);

        let height = chunks[1].height;
        let msgs = self.messages.lock().unwrap();
        let messages: Vec<ListItem> = msgs[0..self.msgs_scroll.min(msgs.len())]
            .iter()
            .rev()
            .map(|m| ListItem::new(Line::from(m)))
            .take(height as usize)
            .collect();
        let messages = List::new(messages)
            .direction(ratatui::widgets::ListDirection::BottomToTop)
            .block(Block::default().borders(Borders::NONE));
	    	//.render(chunks[2], buf);

        let input = Paragraph::new(self.input.value())
            .style(match self.input_mode {
                InputMode::Normal => Style::default(),
                InputMode::Editing => Style::default().fg(Color::Cyan),
                InputMode::Command => Style::default().fg(Color::Yellow),
            })
            .scroll((0, scroll as u16))
            .block(Block::default().borders(Borders::ALL).title("Input2"))//;
	    	.render(chunks[2], buf);






		//render last
        "tui-menu"
            .bold()
            .blue()
            .into_centered_line()
            .render(chunks[0], buf);

        // draw menu last, so it renders on top of other content
        Menu::new().render(chunks[0], buf, &mut self.menu);
    }
}
