/// This example is taken from https://raw.githubusercontent.com/fdehau/tui-rs/master/examples/user_input.rs
//use ratatui::prelude::*;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::{Constraint, Direction, Layout},
    style::Color,
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};

use ratatui::style::Style;
use std::{
    error::Error,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use crate::msg;

#[derive(Default)]
enum InputMode {
    Normal,
    #[default]
    Editing,
}

/// App holds the state of the application
pub struct App {
    /// Current value of the input box
    input: Input,
    /// Current input mode
    input_mode: InputMode,
    /// History of recorded messages
    messages: Arc<Mutex<Vec<msg::Msg>>>,
    _on_input_enter: Option<Box<dyn FnMut(msg::Msg)>>,
    msgs_scroll: usize,
}

impl Default for App {
    fn default() -> Self {
        App {
            input: Input::default(),
            input_mode: InputMode::default(),
            messages: Default::default(),
            _on_input_enter: None,
            msgs_scroll: usize::MAX,
        }
    }
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

    pub fn run(&mut self) -> Result<(), Box<dyn Error>> {
        // setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // run app
        run_app(&mut terminal, self)?;

        // restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        Ok(())
    }
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
    let tick_rate = Duration::from_millis(100);
    loop {
        terminal.draw(|f| ui(f, &app))?;

        if !event::poll(tick_rate)? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('c')
                && key.modifiers.contains(event::KeyModifiers::CONTROL)
            {
                return Ok(());
            }

            match app.input_mode {
                //command prompts
                InputMode::Normal => match key.code {
                    KeyCode::Char(':') => {
                        //if !app.input.value().trim().is_empty() {
                        let m = msg::Msg::default()
                            .set_content(String::from(":command prompt testing..."));
                        app.add_message(m.clone());
                        if let Some(ref mut hook) = app._on_input_enter {
                            hook(m);
                        }
                        //}
                        //app.input.reset();
                    }
                    KeyCode::Char('e') | KeyCode::Char('i') => {
                        app.input_mode = InputMode::Editing;
                        app.msgs_scroll = usize::MAX;
                    }
                    KeyCode::Char('q') | KeyCode::Esc => {
                        return Ok(());
                    }
                    KeyCode::Up => {
                        let l = app.messages.lock().unwrap().len();
                        app.msgs_scroll = app.msgs_scroll.saturating_sub(1).min(l);
                    }
                    KeyCode::Down => {
                        let l = app.messages.lock().unwrap().len();
                        app.msgs_scroll = app.msgs_scroll.saturating_add(1).min(l);
                    }
                    _ => {
                        //TODO command prompts
                        //eval exec
                        //app.input.handle_event(&Event::Key(key));
                    }
                },
                InputMode::Editing => match key.code {
                    KeyCode::Enter => {
                        if !app.input.value().trim().is_empty() {
                            let m = msg::Msg::default().set_content(app.input.value().to_owned());
                            app.add_message(m.clone());
                            if let Some(ref mut hook) = app._on_input_enter {
                                hook(m);
                            }
                        }
                        app.input.reset();
                    }
                    KeyCode::Esc => {
                        app.input_mode = InputMode::Normal;
                        app.msgs_scroll = app.messages.lock().unwrap().len();
                    }
                    _ => {
                        app.input.handle_event(&Event::Key(key));
                    }
                },
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        // .margin(2)
        .constraints(
            [
                Constraint::Length(7),
                Constraint::Fill(5),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(f.size());

    let width = chunks[1].width.max(3) - 3; // keep 2 for borders and 1 for cursor

    let scroll = app.input.visual_scroll(width as usize);

    //HEADER
    //let input = Paragraph::new(app.input.value())
    let header = Paragraph::new("")
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Editing => Style::default().fg(Color::Cyan),
        })
        .scroll((0, scroll as u16))
        .block(Block::default().borders(Borders::ALL).title("HEADER"));
    f.render_widget(header, chunks[0]);
    //HEADER END

    //match app.input_mode {
    //    InputMode::Normal =>
    //        // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
    //        {}

    //    InputMode::Editing => {
    //        // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
    //        f.set_cursor(
    //            // Put cursor past the end of the input text
    //            chunks[1].x + ((app.input.visual_cursor()).max(scroll) - scroll) as u16 + 1,
    //            // Move one line down, from the border to the input line
    //            chunks[1].y + 1,
    //        )
    //    }
    //}

    let height = chunks[1].height;
    let msgs = app.messages.lock().unwrap();
    let messages: Vec<ListItem> = msgs[0..app.msgs_scroll.min(msgs.len())]
        .iter()
        .rev()
        .map(|m| ListItem::new(Line::from(m)))
        .take(height as usize)
        .collect();
    let messages = List::new(messages)
        .direction(ratatui::widgets::ListDirection::BottomToTop)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(messages, chunks[1]);

    let input = Paragraph::new(app.input.value())
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Editing => Style::default().fg(Color::Cyan),
        })
        .scroll((0, scroll as u16))
        .block(Block::default().borders(Borders::ALL).title("Input2"));
    f.render_widget(input, chunks[2]);

    match app.input_mode {
        InputMode::Normal =>
            // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
            {}

        InputMode::Editing => {
            // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
            f.set_cursor(
                // Put cursor past the end of the input text
                chunks[2].x + ((app.input.visual_cursor()).max(scroll) - scroll) as u16 + 1,
                // Move one line down, from the border to the input line
                chunks[2].y + 1,
            )
        }
    }
}
