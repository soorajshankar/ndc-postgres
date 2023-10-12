#!/usr/bin/env bash
set -e -u -o pipefail

EXECUTABLE="$1"
CONNECTION_STRING="$2"
CHINOOK_DEPLOYMENT="$3"

# ensure we clean up
function stop {
  if [[ "${CONFIGURATION_SERVER_PID+x}" ]]; then
    kill "$CONFIGURATION_SERVER_PID"
  fi
  if [[ "${NEW_FILE+x}" ]]; then
    rm -f "$NEW_FILE"
  fi
}

trap stop EXIT

# start the configuration server
cargo run --bin "$EXECUTABLE" --quiet -- configuration serve &
CONFIGURATION_SERVER_PID=$!
./scripts/wait-until --timeout=30 --report -- cargo run --bin "$EXECUTABLE" --quiet -- check-health --port=9100
if ! kill -0 "$CONFIGURATION_SERVER_PID"; then
  echo >&2 'The server stopped abruptly.'
  exit 1
fi

# We want to preserve the connectionUri unchanged in the deployment file, for secrets templating purposes
PRESERVED_DATA="$(jq '{"connectionUri": .connectionUri}' "$CHINOOK_DEPLOYMENT")"

# Native queries should inform the initial configuration call
INITIAL_DATA="$(jq '{"poolSettings": (.poolSettings // {}), "metadata": {"nativeQueries": .metadata.nativeQueries}}' "$CHINOOK_DEPLOYMENT")"

# create a temporary file for the output so we don't overwrite data by accident
NEW_FILE="$(mktemp)"

# 1. Pass the connection string to the configuration server to generate the
#    initial deployment from introspection
# 2. Splice in the preserved data from above
# 3. Format the file
#
# Because we `set -o pipefail` above, this will fail if any of the steps fail,
# and we will abort without overwriting the original file.
curl -fsS http://localhost:9100 \
  | jq --argjson initial_data "$INITIAL_DATA" '. * $initial_data' \
  | jq \
    --arg uri "$CONNECTION_STRING" \
    '. + {"connectionUri": {"uri":$uri}}' \
  | curl -fsS http://localhost:9100 -H 'Content-Type: application/json' -d @- \
  | jq --argjson preserved_data "$PRESERVED_DATA" '. + $preserved_data' \
  | prettier --parser=json \
  > "$NEW_FILE"

# If the above command succeeded, overwrite the original file.
mv -f "$NEW_FILE" "$CHINOOK_DEPLOYMENT"