#!/bin/bash

# Upgrade dependencies but exclude the ones that are patched by risc0.
exec cargo upgrade \
    --exclude sha2 \
    --exclude crypto-bigint \
    --exclude curve25519-dalek \
    --exclude ed25519-dalek \
    "$@"
