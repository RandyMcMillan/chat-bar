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
    #[arg(short = 'g', long)]
    gui: bool,
    #[arg(long = "cfg", default_value = "")]
    config: String,
    #[arg(long = "log_level", default_value = "")]
    log_level: String,
    #[arg(long = "topic", default_value = "")]
    topic: String,
}

/// MyBehaviour
// We create a custom network behaviour that combines Gossipsub and Mdns.
#[derive(NetworkBehaviour)]
pub struct MyBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
}

/// async_prompt
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

/// evt_loop
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

//global_rt
fn global_rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceCell<tokio::runtime::Runtime> = OnceCell::new();
    RT.get_or_init(|| tokio::runtime::Runtime::new().unwrap())
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

//this formats and prints the commit header
fn print_commit_header(app: &TuiApp, commit: &Commit) {
    app.add_commit_message(
        fancy_example::Msg::default()
            .set_content(String::from(format!("commit {}", commit.id())))
            .set_kind(fancy_example::MsgKind::GitCommitHeader),
    );

    if commit.parents().len() > 1 {
        app.add_commit_message(
            fancy_example::Msg::default()
                .set_content(String::from(format!("{}", "Merge:")))
                .set_kind(fancy_example::MsgKind::GitCommitHeader),
        );
        for id in commit.parent_ids() {
            app.add_commit_message(
                fancy_example::Msg::default()
                    .set_content(String::from(format!("{:.8}", id)))
                    .set_kind(fancy_example::MsgKind::GitCommitHeader),
            );
        }
        app.add_commit_message(
            fancy_example::Msg::default()
                .set_content(String::from(format!("{}", "")))
                .set_kind(fancy_example::MsgKind::GitCommitHeader),
        );
    }

    let author = commit.author();
    app.add_commit_message(
        fancy_example::Msg::default()
            .set_content(String::from(format!("Author: {}", author)))
            .set_kind(fancy_example::MsgKind::GitCommitHeader),
    );
    print_time(&app, &author.when(), "Date:   ");
    app.add_commit_message(
        fancy_example::Msg::default()
            .set_content(String::from(format!("{}", "")))
            .set_kind(fancy_example::MsgKind::GitCommitHeader),
    );
}
//this formats and prints the commit header
fn print_commit_body(app: &TuiApp, commit: &Commit) {
    for line in String::from_utf8_lossy(commit.message_bytes()).lines() {
        app.add_commit_message(
            fancy_example::Msg::default()
                .set_content(String::from(format!("    {}", line)))
                .set_kind(fancy_example::MsgKind::GitCommitBody),
        );
    }
}

//called from above
//part of formatting the output
fn print_time(app: &TuiApp, time: &Time, prefix: &str) {
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
    app.add_commit_message(
        fancy_example::Msg::default()
            .set_content(String::from(format!(
                "{}{} {}{:02}{:02}",
                prefix,
                time.strftime("%a %b %e %T %Y").unwrap(),
                sign,
                hours,
                minutes
            )))
            .set_kind(fancy_example::MsgKind::GitCommitTime),
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
//fn main() -> () {
    //TuiApp begin
    let mut tui_app = TuiApp::default();

    //repo
    let repo = get_repo().expect("get_repo() falied!");

    //env
    let args_vec: Vec<String> = env_args().collect();
    debug!("Arguments:");
    for (index, arg) in args_vec.iter().enumerate() {
        if Some(index) == Some(0) {
            debug!("Some(index) = Some(0):  {}: {}", index, arg);
        } else {
            debug!("  {}: {}", index, arg);
        }
    }

    let cli_args = Args::parse();
    for _ in 0..cli_args.count {
        println!("Hello {}!", cli_args.name);
    }

    println!("cli_args.log_level {}!", cli_args.log_level.clone());
    if cli_args.log_level.len() > 0 {
        debug!("log_level {}!", cli_args.log_level.clone());

        Builder::from_env(Env::default().default_filter_or(
            cli_args.log_level.clone()
                + ",libp2p_gossipsub::behaviour=error,eframe=error,egui_glow=error,egui_winit=error,egui_extras=error",
        ))
        .init();
    } else {
        Builder::from_env(Env::default().default_filter_or(
            "info,libp2p_gossipsub::behaviour=error,eframe=error,egui_glow=error,egui_winit=error,egui_extras=error",
        ))
        .init();
    }

    println!("cli_args.tui {}!", cli_args.tui.clone());
    if cli_args.tui {
        // Get the reference to HEAD
        let head = repo.head().expect("repo.head failed!");
        println!("HEAD: {}", head.name().unwrap_or("HEAD"));

	    let commit = head.peel_to_commit().expect("head.peel_to_commit");
        // print_commit_header(&app, &commit);
        // Print the commit ID (SHA-1 hash)
        println!("Commit ID: {}", commit.id());
        println!("Commit Summary: {:?}", commit.summary());


        let (peer_tx, mut peer_rx) = tokio::sync::mpsc::channel::<Msg>(100);
        let (input_tx, input_rx) = tokio::sync::mpsc::channel::<Msg>(100);

        // let input_loop_fut = input_loop(input_tx);
        let input_tx_clone = input_tx.clone();
        tui_app.on_submit(move |m| {
            debug!("sent: {:?}", m);
            input_tx_clone.blocking_send(m).unwrap();
        });

        //topic
        //println!("cli_args.topic {}!", cli_args.topic);
        let topic;
        if cli_args.topic.len() > 0 {
            topic = String::from(format!("{}", cli_args.topic.clone()));

            //let search_oid = Oid::from_str("your_commit_oid_here")?; // Replace with the commit OID you're looking for.

            let mut revwalk = repo.revwalk().expect("revwalk");
            revwalk.push_head().expect("revwalk.push_head"); // Start from HEAD
            revwalk
                .set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
                .expect("revwalk.set_sorting"); // Order commits

            //for oid in revwalk {
            let search_oid = Oid::from_str(&topic.clone()).unwrap();
            let commit = repo.find_commit(search_oid).expect("repo.find_commit");
            if commit.id() == search_oid {
                tui_app.add_message(
                    Msg::default()
                        .set_content(String::from(format!("Found commit: {}", commit.id())))
                        .set_kind(MsgKind::GitCommitHeader),
                );
                tui_app.add_message(
                    Msg::default()
                        .set_content(String::from(format!("Found commit: {}", commit.author())))
                        .set_kind(MsgKind::GitCommitHeader),
                );
                tui_app.add_message(
                    Msg::default()
                        .set_content(String::from(format!(
                            "Found commit: {:?}",
                            commit.summary().unwrap()
                        )))
                        .set_kind(MsgKind::GitCommitHeader),
                );
            } else {
                tui_app.add_message(
                    Msg::default()
                        .set_content(String::from(format!(
                            "----Commit: {} not found.",
                            commit.id()
                        )))
                        .set_kind(MsgKind::GitCommitHeader),
                );
            }
            //}

            tui_app.topic = topic.clone();
        } else {
            //topic = String::from(format!("{:0>64}", 0));
            //for line in String::from_utf8_lossy(commit.message_bytes()).lines() {
            //    let message = Msg::default()
            //        //no! .set_content(format!("{:?}", line))
            //        .set_content(format!("{:}", line))
            //        .set_kind(MsgKind::Git);
            //    tui_app.add_message(message);
            //}
            topic = String::from(format!("{}", commit.id()));
            tui_app.topic = topic.clone();
            //tui_app.add_message(
            //    Msg::default()
            //        .set_content(topic.clone())
            //        .set_kind(MsgKind::Chat),
            //);
            print_commit_header(&tui_app, &commit);
            print_commit_body(&tui_app, &commit);
        }

        //debug!("{}", topic);
        let topic = gossipsub::IdentTopic::new(format!("{}", topic));
        //debug!("{}", topic);
        global_rt().spawn(async move {
            evt_loop(input_rx, peer_tx, topic).await.unwrap();
        });
        //topic

        // recv from peer
        let mut tui_msg_adder = tui_app.add_msg_fn();
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

		let mut terminal = init_terminal().expect("init_terminal() failed!");
        //app.run
        tui_app.run(&mut terminal).expect("tui_app.run");

        // say goodbye
        input_tx.blocking_send(Msg::default().set_kind(MsgKind::Leave)).expect("input_tx.blocking_send");
        std::thread::sleep(Duration::from_millis(500));

        // restore terminal
        disable_raw_mode().expect("disable_raw_mode");
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        ).expect("execute!terminal backend");
        terminal.show_cursor().expect("tereminal.show_cursor");
        restore_terminal().expect("restore_terminal");
        return Ok(());
    }

    //TuiApp end
    //GuiApp begin
    //GuiApp begin
    debug!("cli_args.gui {}!", cli_args.gui.clone());
    if cli_args.gui {
        //use eframe::NativeOptions;
        //env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

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

        return eframe::run_native(
            "Dnd Example App",
			eframe::NativeOptions::default(),
            Box::new(move |ctx| Ok(Box::new(App::new(&ctx.egui_ctx)) as Box<dyn eframe::App>)),
        )
	} else {
		return Ok(())
	}
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
