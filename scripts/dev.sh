#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

export AIO_PLUGIN_FRONTEND="${AIO_PLUGIN_FRONTEND:-frontend}"
export HOST=127.0.0.1
cargo build --locked
exec target/debug/aio-plugin-tv-agent
