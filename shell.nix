{ pkgs ? import <nixpkgs> {} }:

let
  fenix = import (fetchTarball "https://github.com/nix-community/fenix/archive/main.tar.gz") {};
  rustToolchain = fenix.complete.toolchain;
in
pkgs.mkShell {
  buildInputs = [
    # Rust toolchain (nightly from fenix for let chains / 2024 edition)
    rustToolchain

    # Build dependencies
    pkgs.pkg-config

    # Audio (cpal/alsa-sys)
    pkgs.alsa-lib

    # HTTP/TLS (last-fm-rs)
    pkgs.openssl
  ];

  RUST_BACKTRACE = "1";

  shellHook = ''
    echo ""
    echo "Shelltrax Development Environment"
    echo "=================================="
    echo "Rust: $(rustc --version)"
    echo "Cargo: $(cargo --version)"
    echo ""
    echo "Commands:"
    echo "  cargo build    - Build the project"
    echo "  cargo run      - Run shelltrax"
    echo "  cargo test     - Run tests"
    echo "  cargo clippy   - Lint"
    echo ""
  '';
}
