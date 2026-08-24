FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

RUN cargo install cargo-watch --locked \
    && cargo install cargo-release --locked \
    && chown -R vscode:vscode /usr/local/cargo
