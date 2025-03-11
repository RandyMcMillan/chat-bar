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
use fancy_example::*;

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

////global_rt
//fn global_rt() -> &'static tokio::runtime::Runtime {
//    static RT: OnceCell<tokio::runtime::Runtime> = OnceCell::new();
//    RT.get_or_init(|| tokio::runtime::Runtime::new().unwrap())
//}
//
pub fn get_repo() -> color_eyre::Result<Repository> {
    Ok(Repository::discover(".")?)
}
//
//fn split_strings_in_vec(vec: Vec<String>, delimiter: char) -> Vec<Vec<String>> {
//    vec.into_iter()
//        .map(|s| s.split(delimiter).map(|s| s.to_string()).collect())
//        .collect()
//}
//
//fn split_into_chunks(vec: Vec<String>, chunk_size: usize) -> Vec<Vec<String>> {
//    vec.chunks(chunk_size).map(|chunk| chunk.to_vec()).collect()
//}
//
////this formats and prints the commit header
//fn print_commit_header(app: &TuiApp, commit: &Commit) {
//    app.add_commit_message(
//        fancy_example::Msg::default()
//            .set_content(String::from(format!("commit {}", commit.id())))
//            .set_kind(fancy_example::MsgKind::GitCommitHeader),
//    );
//
//    if commit.parents().len() > 1 {
//        app.add_commit_message(
//            fancy_example::Msg::default()
//                .set_content(String::from(format!("{}", "Merge:")))
//                .set_kind(fancy_example::MsgKind::GitCommitHeader),
//        );
//        for id in commit.parent_ids() {
//            app.add_commit_message(
//                fancy_example::Msg::default()
//                    .set_content(String::from(format!("{:.8}", id)))
//                    .set_kind(fancy_example::MsgKind::GitCommitHeader),
//            );
//        }
//        app.add_commit_message(
//            fancy_example::Msg::default()
//                .set_content(String::from(format!("{}", "")))
//                .set_kind(fancy_example::MsgKind::GitCommitHeader),
//        );
//    }
//
//    let author = commit.author();
//    app.add_commit_message(
//        fancy_example::Msg::default()
//            .set_content(String::from(format!("Author: {}", author)))
//            .set_kind(fancy_example::MsgKind::GitCommitHeader),
//    );
//    print_time(&app, &author.when(), "Date:   ");
//    app.add_commit_message(
//        fancy_example::Msg::default()
//            .set_content(String::from(format!("{}", "")))
//            .set_kind(fancy_example::MsgKind::GitCommitHeader),
//    );
//}
////this formats and prints the commit header
//fn print_commit_body(app: &TuiApp, commit: &Commit) {
//    for line in String::from_utf8_lossy(commit.message_bytes()).lines() {
//        app.add_commit_message(
//            fancy_example::Msg::default()
//                .set_content(String::from(format!("    {}", line)))
//                .set_kind(fancy_example::MsgKind::GitCommitBody),
//        );
//    }
//}
//
////called from above
////part of formatting the output
//fn print_time(app: &TuiApp, time: &Time, prefix: &str) {
//    let (offset, sign) = match time.offset_minutes() {
//        n if n < 0 => (-n, '-'),
//        n => (n, '+'),
//    };
//    let (hours, minutes) = (offset / 60, offset % 60);
//    let ts = time::Timespec::new(time.seconds() + (time.offset_minutes() as i64) * 60, 0);
//    let time = time::at(ts);
//
//    println!(
//        "{}{} {}{:02}{:02}",
//        prefix,
//        time.strftime("%a %b %e %T %Y").unwrap(),
//        sign,
//        hours,
//        minutes
//    );
//    app.add_commit_message(
//        fancy_example::Msg::default()
//            .set_content(String::from(format!(
//                "{}{} {}{:02}{:02}",
//                prefix,
//                time.strftime("%a %b %e %T %Y").unwrap(),
//                sign,
//                hours,
//                minutes
//            )))
//            .set_kind(fancy_example::MsgKind::GitCommitTime),
//    );
//}


#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {


    //TuiApp begin
    //let mut terminal = init_terminal().expect("init_terminal() falied!");
    let mut app = TuiApp::default();

    //repo
    let repo = get_repo().expect("get_repo() falied!");


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


	//TuiApp end

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
