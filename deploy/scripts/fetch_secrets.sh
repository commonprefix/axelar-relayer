#!/bin/bash
set -e

WORKSPACE="$1"
ACCOUNT_ID="$2"
PROFILE="${3:-default}"   # 3rd parameter or "default" if not set

if [ -z "$WORKSPACE" ] || [ -z "$ACCOUNT_ID" ]; then
  echo "Error: WORKSPACE and ACCOUNT_ID parameters are required"
  echo "Usage: $0 <WORKSPACE> <ACCOUNT_ID> [PROFILE]"
  exit 1
fi

CREDENTIALS=$(aws sts assume-role \
  --profile "$PROFILE" \
  --role-arn "arn:aws:iam::${ACCOUNT_ID}:role/relayer-role-${WORKSPACE}" \
  --role-session-name "DockerSession")

export AWS_ACCESS_KEY_ID=$(echo "$CREDENTIALS" | jq -r '.Credentials.AccessKeyId')
export AWS_SECRET_ACCESS_KEY=$(echo "$CREDENTIALS" | jq -r '.Credentials.SecretAccessKey')
export AWS_SESSION_TOKEN=$(echo "$CREDENTIALS" | jq -r '.Credentials.SessionToken')
export AWS_DEFAULT_REGION="us-east-1"

CONFIG_PATH="/app/relayer_base/config/config.${WORKSPACE}.yaml"
CERT_PATH="/app/relayer_base/certs/client.crt"
KEY_PATH="/app/relayer_base/certs/client.key"

mkdir -p "$(dirname "$CONFIG_PATH")" "$(dirname "$CERT_PATH")"

aws secretsmanager get-secret-value \
  --secret-id "relayer/${WORKSPACE}-config" \
  --query 'SecretString' \
  --output text > "$CONFIG_PATH"

aws secretsmanager get-secret-value \
  --secret-id "relayer/${WORKSPACE}-certificate" \
  --query 'SecretString' \
  --output text > "$CERT_PATH"

aws secretsmanager get-secret-value \
  --secret-id "relayer/${WORKSPACE}-key" \
  --query 'SecretString' \
  --output text > "$KEY_PATH"

echo "✅ Secrets written to:"
echo "  $CONFIG_PATH"
echo "  $CERT_PATH"
echo "  $KEY_PATH"