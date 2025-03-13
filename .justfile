default:
    @just --list

chat:
    @cargo b --manifest-path Cargo.toml
    @USER=$(gnostr-weeble)/$(gnostr-blockheight)/$(gnostr-wobble)/${USER} cargo run --manifest-path Cargo.toml --bin gnostr-chat

chat-install:
    @cargo install --path .

cargo-binstall:
    @curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash

cargo-dist:
    @just cargo-binstall
    @cargo binstall cargo-dist@0.25.1

gnostr-bins:
    @just cargo-binstall
    @cargo binstall gnostr-bins
