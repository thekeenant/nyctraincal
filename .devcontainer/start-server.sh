#!/bin/sh

export CARGO_HOME=/home/vscode/.cargo

if ! pgrep -f '^cargo watch -x run$' >/dev/null; then
    nohup cargo watch -x run >/tmp/nyctraincal-dev.log 2>&1 &
fi
