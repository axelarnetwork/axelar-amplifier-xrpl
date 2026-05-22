#!/usr/bin/env bash
set -euo pipefail

MDBOOK_VERSION="0.4.40"
MERMAID_VERSION="0.14.0"

curl -sSL "https://github.com/rust-lang/mdBook/releases/download/v${MDBOOK_VERSION}/mdbook-v${MDBOOK_VERSION}-x86_64-unknown-linux-gnu.tar.gz" | tar -xz
curl -sSL "https://github.com/badboy/mdbook-mermaid/releases/download/v${MERMAID_VERSION}/mdbook-mermaid-v${MERMAID_VERSION}-x86_64-unknown-linux-gnu.tar.gz" | tar -xz

PATH="$PWD:$PATH" ./mdbook build doc
