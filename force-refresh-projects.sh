#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${1:-http://127.0.0.1:3000}"
URL="${BASE_URL%/}/api/projects/refresh"

response="$(curl --silent --show-error --write-out $'\n%{http_code}' --request POST "$URL" || true)"
body="${response%$'\n'*}"
status="${response##*$'\n'}"

if [[ "$status" == "405" ]]; then
  curl --fail --silent --show-error --request GET "$URL"
elif [[ "$status" =~ ^2[0-9][0-9]$ ]]; then
  printf '%s' "$body"
else
  printf '%s\n' "$body" >&2
  echo "refresh request failed with HTTP $status" >&2
  exit 1
fi
echo
