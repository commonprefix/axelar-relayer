#!/usr/bin/env bash
#
# Usage: ./install.sh <SUFFIX>
#
# Example:
#   ./install.sh devnet
#
# This will copy the following files:
#   target/release/xrpl_subscriber    -> /usr/local/bin/xrpl_subscriber-devnet
#   target/release/xrpl_distributor  -> /usr/local/bin/xrpl_distributor-devnet
#   target/release/xrpl_ingestor     -> /usr/local/bin/xrpl_ingestor-devnet
#   target/release/xrpl_includer     -> /usr/local/bin/xrpl_includer-devnet

# Exit if any command fails
set -e

if [ -z "$1" ]; then
  echo "Error: No network argument provided."
  echo "Usage: ./install.sh <NETWORK>"
  exit 1
fi

NETWORK="$1"

# Stop services before copying files
sudo systemctl stop xrpl-{subscriber,includer,ingestor,distributor,ticket-creator}@${NETWORK}-{0,1,2,3}.service

for bin in subscriber distributor ingestor includer ticket_creator
do
    sudo cp "target/release/xrpl_${bin}" "/usr/local/bin/xrpl_${bin}_${NETWORK}"
done

echo "Successfully installed ${NETWORK} relayer services."

# Start services based on the NETWORK value
if [ "$NETWORK" = "devnet" ]; then
  sudo systemctl start xrpl-{subscriber,includer,distributor,ticket-creator}@${NETWORK}-0.service
  sudo systemctl start xrpl-ingestor@${NETWORK}-{0,1}.service
elif [ "$NETWORK" = "testnet" ]; then
  sudo systemctl start xrpl-{subscriber,distributor,ticket-creator}@${NETWORK}-0.service
  sudo systemctl start xrpl-ingestor@${NETWORK}-{0,1,2,3}.service
  sudo systemctl start xrpl-includer@${NETWORK}-{0,1}.service
else
  echo "No systemctl start commands defined for network '$NETWORK'."
fi

echo "Successfully restarted ${NETWORK} relayer services."