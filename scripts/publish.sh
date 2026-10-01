#!/usr/bin/env bash
# Publish deckr to crates.io.
#
# Usage: ./scripts/publish.sh            (prompts for the token, hidden)
#    or: CARGO_REGISTRY_TOKEN=xxx ./scripts/publish.sh
#
# The token is read from the environment or from a hidden prompt — never from
# argv — so it does not end up in shell history. The managed toolchain is used
# directly because the `cargo` on PATH is a broken stub on this machine.
#
# The first release of a crate name always needs a token: crates.io has no web
# upload, and trusted publishing can only be bound to a crate that already
# exists. After that first publish, configure trusted publishing in the crate
# settings and this script is no longer necessary — a tag publishes itself.
set -euo pipefail

cd "$(dirname "$0")/.."

TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable-x86_64-pc-windows-gnu}"
CARGO="${CARGO_BIN:-$HOME/.rustup/toolchains/$TOOLCHAIN/bin/cargo.exe}"
if [ ! -x "$CARGO" ]; then
    CARGO="cargo"
fi

if [ -z "${CARGO_REGISTRY_TOKEN:-}" ]; then
    printf 'crates.io token (hidden): '
    read -rs CARGO_REGISTRY_TOKEN
    printf '\n'
    export CARGO_REGISTRY_TOKEN
fi
if [ -z "$CARGO_REGISTRY_TOKEN" ]; then
    echo "error: no token given" >&2
    exit 1
fi

export RUSTUP_TOOLCHAIN="$TOOLCHAIN"
export RUSTC="${RUSTC:-$HOME/.rustup/toolchains/$TOOLCHAIN/bin/rustc.exe}"

echo "==> verifying the package builds on its own"
"$CARGO" package --locked
echo "==> publishing deckr $("$CARGO" metadata --no-deps --format-version 1 \
    | tr ',' '\n' | grep -m1 '"version"' | cut -d'"' -f4) to crates.io"
"$CARGO" publish --locked
