#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

if [ -f .env.local ]; then
  set -a
  . ./.env.local
  set +a
fi

cargo test --locked
cargo zigbuild --locked --release \
  --target x86_64-unknown-linux-gnu.2.17

rm -rf dist/frontend
mkdir -p dist/frontend dist/android
cp -R frontend/. dist/frontend/
cp target/x86_64-unknown-linux-gnu/release/aio-plugin-tv-agent dist/server
chmod 0755 dist/server

(cd android && ./gradlew assembleDebug)
cp android/app/build/outputs/apk/debug/app-debug.apk dist/android/tv-agent-debug.apk
