export LIBP2P_MDNS_SERVICE_NAME = _bar._tcp.local

USER:=$(shell date +%s%3)
export USER

gnostr_chat:
	@echo "Running gnostr-chat"
	@USER=${USER} cargo run --bin gnostr-chat

chat_user:
	@USER=${USER} cargo run --bin gnostr-chat -- --topic chat_user

basic:
	@echo "Running basic"
	cargo run --bin basic

nested_groups:
	@echo "Running nested_groups"
	cargo run --bin nested_groups

build:
	@echo "Building chat_bar"
	cargo build --release

install:
	@echo "Installing chat_bar"
	cargo install --path .

docs:
	@echo "generating docs"
	cargo d
