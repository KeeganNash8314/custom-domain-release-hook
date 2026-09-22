#!/bin/sh
set -eu

: "${INFRAI_WEBHOOK_SECRET:?set INFRAI_WEBHOOK_SECRET}"
: "${1:?usage: ./scripts/run-local.sh BUILD_ID DOMAIN}"
: "${2:?usage: ./scripts/run-local.sh BUILD_ID DOMAIN}"
cargo run --bin custom-domainctl -- serve "$1" "$2"
