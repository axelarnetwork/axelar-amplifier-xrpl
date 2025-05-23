#!/usr/bin/env bash
set -e
set -x

TARGET=$1
TARGET="${TARGET/-/_}"
MY_DIR="$(dirname "$(realpath "$0")")"
OUTPUT_DIR="$MY_DIR/artifacts"

if ! which wasm-opt >/dev/null 2>&1; then
    echo "Error: wasm-opt is not installed. Please install it and try again."
    echo "Error: Check this repo https://github.com/WebAssembly/binaryen for wasm-opt"
    exit 1
fi

mkdir -p "$OUTPUT_DIR"
wasm-opt -Oz -o "$OUTPUT_DIR/$TARGET.wasm" "$MY_DIR/target/wasm32-unknown-unknown/release/$TARGET.wasm"
