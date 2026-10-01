#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

cargo test --locked
cargo zigbuild --locked --release \
  --target x86_64-unknown-linux-gnu.2.17

rm -rf dist/frontend
mkdir -p dist/frontend
cp -R frontend/. dist/frontend/
cp target/x86_64-unknown-linux-gnu/release/aio-plugin-tv-agent dist/server
chmod 0755 dist/server
