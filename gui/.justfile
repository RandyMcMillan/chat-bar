default:
    @DATE=$(date +%s)
    @just --choose
fancy-example-gui:
    @USER=$(date +%s)/${USER} cargo run --manifest-path fancy-example/Cargo.toml -- -g --log_level none
fancy-example-tui:
    @USER=$(date +%s)/${USER} cargo run --manifest-path fancy-example/Cargo.toml -- -t --log_level none


date_seconds:
  @echo "Date in seconds from environment: $(date +%s)"
