default:
    @just --list
fancy-example-gui:
    @USER=fancy-example-gui cargo run --manifest-path fancy-example/Cargo.toml -- -g --log_level none
fancy-example-tui:
    @USER=fancy-example-tui cargo run --manifest-path fancy-example/Cargo.toml -- -t --log_level none
