#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${1:-http://127.0.0.1:3000}"
curl --fail --silent --show-error --request POST "${BASE_URL%/}/api/projects/refresh"
echo
