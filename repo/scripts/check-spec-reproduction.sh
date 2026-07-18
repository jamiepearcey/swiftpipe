#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "$0")/.."

echo "== format check =="
cargo fmt --all -- --check

echo "== reproduction tests =="
cargo test -p swift-schema --test spec_reproduction
cargo test -p swift-schema --test uhb_spec_parser

echo "== schema validation =="
cargo run -p swift-cli -- schema validate examples/schemas
cargo run -p swift-cli -- schema render-validate examples/schemas

echo "== schema coverage =="
cargo run -p swift-cli -- schema coverage examples/schemas

echo "== full schema test suite =="
cargo test -p swift-schema

echo "spec reproduction checks passed"
