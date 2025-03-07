default:
    @just --list
chat:
    @cargo b --manifest-path chat-bar/Cargo.toml
    @cargo run --manifest-path chat-bar/Cargo.toml --bin chat-bar
chat-install:
    @cargo install --path chat-bar
