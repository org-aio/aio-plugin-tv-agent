#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

if [ -f .env.local ]; then
  set -a
  . ./.env.local
  set +a
fi

export AIO_PLUGIN_FRONTEND="${AIO_PLUGIN_FRONTEND:-frontend}"
export HOST=127.0.0.1
cargo build --locked
exec target/debug/aio-plugin-tv-agent
