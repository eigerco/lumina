# Rust Celestia node

## Running Go celestia node for integration

Follow [this guide](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry#authenticating-with-a-personal-access-token-classic)
to authorize yourself in github's container registry.

Start a celestia network with single validator and bridge
```bash
docker-compose -f ci/docker-compose.yml up --build --force-recreate -d
```

When deleting, remember to also delete subvolumes:
```bash
docker-compose -f ci/docker-compose.yml down -v
```

To get the JWT token for the account with coins (coins will be transferred in block 2):
```bash
export CELESTIA_NODE_AUTH_TOKEN=$(docker-compose -f ci/docker-compose.yml exec bridge celestia bridge auth admin --p2p.network private)
```

Accessing json RPC api with `celestia` cli:
```bash
celestia rpc blob Submit 0x0c204d39600fddd3 '"Hello world"' --print-request
```
