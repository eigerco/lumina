#!/bin/bash

set -euo pipefail

# Amount of bridge nodes to setup, taken from the first argument
# or 1 if not provided
BRIDGE_COUNT="${BRIDGE_COUNT:-1}"
# a private local network
P2P_NETWORK="private"
# a validator node configuration directory
CONFIG_DIR="$CELESTIA_HOME/.celestia-app"
# the names of the keys
NODE_NAME=validator-0
# amounts of the coins for the keys
BRIDGE_COINS="200000000000000utia"
VALIDATOR_COINS="1000000000000000utia"
# a directory and the files shared with the bridge nodes
CREDENTIALS_DIR="/credentials"
# directory where validator will write the genesis hash
GENESIS_DIR="/genesis"
GENESIS_HASH_FILE="$GENESIS_DIR/genesis_hash"

# Get the address of the node of given name
node_address() {
  local node_name="$1"
  local node_address

  node_address=$(celestia-appd keys show "$node_name" -a --keyring-backend="test")
  echo "$node_address"
}

# Waits for the given block to be created and returns it's hash
wait_for_block() {
  local block_num="$1"
  local block_hash=""

  # Wait for the block to be created
  while [[ -z "$block_hash" ]]; do
    # `|| echo` fallbacks to an empty string in case it's not ready
    block_hash="$(celestia-appd query block "$block_num" 2>/dev/null | jq '.block_id.hash' || echo)"
    sleep 0.1
  done

  echo "$block_hash"
}

# Saves the hash of the genesis node and the keys funded with the coins
# to the directory shared with the bridge node
provision_bridge_nodes() {
  local genesis_hash
  local last_node_idx=$((BRIDGE_COUNT - 1))

  # Save the genesis hash for the bridge
  genesis_hash=$(wait_for_block 1)
  echo "Saving a genesis hash to $GENESIS_HASH_FILE"
  echo "$genesis_hash" > "$GENESIS_HASH_FILE"

  # Get or create the keys for bridge nodes
  for node_idx in $(seq 0 "$last_node_idx"); do
    local bridge_name="bridge-$node_idx"
    local key_file="$CREDENTIALS_DIR/$bridge_name.key"
    local plaintext_key_file="$CREDENTIALS_DIR/$bridge_name.plaintext-key"
    local addr_file="$CREDENTIALS_DIR/$bridge_name.addr"

    if [ ! -e "$key_file" ]; then
      # if key don't exist yet, then create and export it
      echo "Creating a new keys for the $bridge_name"
      celestia-appd keys add "$bridge_name" --keyring-backend "test"
      # export it
      echo "password" | celestia-appd keys export "$bridge_name" 2> "$key_file.lock"
      # export also plaintext key for convenience in tests
      echo y | celestia-appd keys export "$bridge_name" --unsafe --unarmored-hex 2> "${plaintext_key_file}"
      # the `.lock` file and `mv` ensures that readers read file only after finished writing
      mv "$key_file.lock" "$key_file"
      # export associated address
      node_address "$bridge_name" > "$addr_file"
    else
      # otherwise, just import it
      echo "password" | celestia-appd keys import "$bridge_name" "$key_file" \
        --keyring-backend="test"
    fi
  done

  # Transfer the coins to bridge nodes addresses
  # Coins transfer need to be after validator registers EVM address, which happens in block 2.
  # see `setup_private_validator`
  local start_block=3

  for node_idx in $(seq 0 "$last_node_idx"); do
    # TODO: create an issue in celestia-app and link it here
    # we need to transfer the coins for each node in separate
    # block, or the signing of all but the first one will fail
    wait_for_block $((start_block + node_idx))

    local bridge_name="bridge-$node_idx"
    local bridge_address

    bridge_address=$(node_address "$bridge_name")

    echo "Transfering $BRIDGE_COINS coins to the $bridge_name"
    echo "y" | celestia-appd tx bank send \
      "$NODE_NAME" \
      "$bridge_address" \
      "$BRIDGE_COINS" \
      --fees 21000utia
  done

  echo "Provisioning finished."
}

# Set up the validator for a private alone network.
# Based on
# https://github.com/celestiaorg/celestia-app/blob/main/scripts/single-node.sh
setup_private_validator() {
  local validator_addr

  # Initialize the validator
  celestia-appd init "$P2P_NETWORK" --chain-id "$P2P_NETWORK"
  # Derive a new private key for the validator
  celestia-appd keys add "$NODE_NAME" --keyring-backend="test"
  validator_addr="$(celestia-appd keys show "$NODE_NAME" -a --keyring-backend="test")"
  # Create a validator's genesis account for the genesis.json with an initial bag of coins
  celestia-appd add-genesis-account "$validator_addr" "$VALIDATOR_COINS"
  # Generate a genesis transaction that creates a validator with a self-delegation
  celestia-appd gentx "$NODE_NAME" 5000000000utia \
    --fees 500utia \
    --keyring-backend="test" \
    --chain-id "$P2P_NETWORK"
  # Collect the genesis transactions and form a genesis.json
  celestia-appd collect-gentxs

  # Set proper defaults and change ports
  # If you encounter: `sed: -I or -i may not be used with stdin` on MacOS you can mitigate by installing gnu-sed
  # https://gist.github.com/andre3k1/e3a1a7133fded5de5a9ee99c87c6fa0d?permalink_comment_id=3082272#gistcomment-3082272
  sed -i'.bak' 's|"tcp://127.0.0.1:26657"|"tcp://0.0.0.0:26657"|g' "$CONFIG_DIR/config/config.toml"
  # enable transaction indexing
  sed -i'.bak' 's|indexer = .*|indexer = "kv"|g' "$CONFIG_DIR/config/config.toml"
}

main() {
  # Configure stuff
  setup_private_validator
  # Spawn a job to provision a bridge node later
  provision_bridge_nodes &
  # Start the celestia-app
  echo "Configuration finished. Running a validator node..."
  celestia-appd start --api.enable --grpc.enable --force-no-bbr
}

main
