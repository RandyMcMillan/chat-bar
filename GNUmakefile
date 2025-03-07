default:
	@just --list || make chat
chat:
	@cargo b --manifest-path chat-bar/Cargo.toml
	@cargo run --manifest-path chat-bar/Cargo.toml --bin chat_bar
chat-install:
	@cargo install --path chat-bar
