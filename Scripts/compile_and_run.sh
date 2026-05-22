#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
source "$ROOT/project.env"
pkill -x "$APP_NAME" 2>/dev/null || true
Scripts/package_app.sh
open "$APP_NAME.app"
