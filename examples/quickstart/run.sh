#!/usr/bin/env bash
set -euo pipefail

BIN=${1:-bunker-convert}
shift || true

"${BIN}" run recipes/quickstart-webp.yaml "$@"
