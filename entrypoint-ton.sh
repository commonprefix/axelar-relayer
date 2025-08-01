#!/bin/sh
set -e

cmd="$1"
shift

if command -v "$cmd" >/dev/null 2>&1; then
  exec "$cmd" "$@"
else
  echo "Unknown binary: $cmd"
  exit 1
fi