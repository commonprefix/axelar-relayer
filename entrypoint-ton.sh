#!/bin/sh
set -e
cmd="$1"
shift

if [ -n "$WORKSPACE" ] && [ -n "$AWS_ACCOUNT_ID" ]; then
  if [ -n "$AWS_PROFILE" ]; then
    ./fetch_secrets.sh "$WORKSPACE" "$AWS_ACCOUNT_ID" "$AWS_PROFILE"
  else
    ./fetch_secrets.sh "$WORKSPACE" "$AWS_ACCOUNT_ID"
  fi
else
  echo "ℹ️ WORKSPACE or AWS_ACCOUNT_ID is not set, skipping fetch_secrets"
fi

if command -v "$cmd" >/dev/null 2>&1; then
  exec "$cmd" "$@"
else
  echo "Unknown binary: $cmd"
  exit 1
fi