#!/usr/bin/env bash
set -euo pipefail

cargo build --release "$@"
cargo run -- security sbom --output target/bunker-convert-sbom.json
cargo run -- security digest --path target/release/bunker-convert --output target/bunker-convert.sha256

echo "Artifacts written to target/"
