default:
	@just --list || make chat
chat:
	@cargo b --manifest-path Cargo.toml
	@cargo run --manifest-path Cargo.toml --bin gnostr-chat
chat-install:
	@cargo install --path .
