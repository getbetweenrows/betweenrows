#!/usr/bin/env bash
# setup.sh — end-to-end bootstrap for the canonical demo_ecommerce demo.
#
# Runs in three phases against a fresh BR proxy + upstream postgres stack:
#   1. Upstream data — schema.sql + seed.py
#   2. BR admin bootstrap — tenant attribute, alice/bob/charlie, prod-db
#      datasource, save catalog
#   3. Policies + access — create policies from policies.yaml, grant user
#      access, assign tenant-isolation policies with scope=all
#
# Phase 2/3 create calls are idempotent (duplicates are ignored via || true
# or by looking up existing records). Phase 1 is NOT idempotent — seed.py
# will duplicate customer/order/product rows on re-run. To reset:
#   docker compose -f compose.demo.yaml down -v
# then re-run this script.
#
# Prerequisites on the host: psql, python3 (with requirements.txt
# installed), curl, jq. The BR proxy and upstream postgres must be
# reachable at $BR_HOST and $UPSTREAM_DSN.
#
# Usage:
#   cd scripts/demo_ecommerce
#   docker compose -f compose.demo.yaml up -d
#   pip install -r requirements.txt
#   ./setup.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# --- Config (override via env) --------------------------------------------
BR_HOST="${BR_HOST:-http://127.0.0.1:5435}"
BR_ADMIN_USER="${BR_ADMIN_USER:-admin}"
BR_ADMIN_PASSWORD="${BR_ADMIN_PASSWORD:-changeme}"

# DSN psql/seed.py uses from the host to hit the upstream postgres.
UPSTREAM_DSN="${UPSTREAM_DSN:-postgresql://postgres:postgres@127.0.0.1:5432/demo_ecommerce}"

# Datasource config the BR proxy uses to reach the upstream. Inside the
# compose network, the proxy reaches upstream at `upstream:5432`; from the
# host it's `127.0.0.1:5432`. Default here assumes the compose.demo.yaml
# network, override BR_UPSTREAM_HOST for other setups.
BR_UPSTREAM_HOST="${BR_UPSTREAM_HOST:-upstream}"
BR_UPSTREAM_PORT="${BR_UPSTREAM_PORT:-5432}"
BR_UPSTREAM_DB="${BR_UPSTREAM_DB:-demo_ecommerce}"
BR_UPSTREAM_USER="${BR_UPSTREAM_USER:-postgres}"
BR_UPSTREAM_PASS="${BR_UPSTREAM_PASS:-postgres}"

# BR datasource name that users connect to through the proxy.
DATASOURCE_NAME="${DATASOURCE_NAME:-demo_ecommerce}"

# --- Helpers ---------------------------------------------------------------
log() { printf '\033[36m→\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

require_cmd curl
require_cmd jq
require_cmd psql
require_cmd python3

auth() { curl -fsS -H "Authorization: Bearer $TOKEN" "$@"; }

# --- Phase 1: upstream data -----------------------------------------------
log "Phase 1 — applying schema.sql and running seed.py"
psql "$UPSTREAM_DSN" -v ON_ERROR_STOP=1 -q -f "$SCRIPT_DIR/schema.sql" >/dev/null
DATABASE_URL="$UPSTREAM_DSN" python3 "$SCRIPT_DIR/seed.py"

# --- Phase 2: BR admin bootstrap ------------------------------------------
log "Phase 2 — logging into BR admin at $BR_HOST"
LOGIN_BODY=$(printf '{"username":"%s","password":"%s"}' \
  "$BR_ADMIN_USER" "$BR_ADMIN_PASSWORD")
TOKEN=$(curl -fsS -X POST "$BR_HOST/api/v1/auth/login" \
  -H 'Content-Type: application/json' \
  -d "$LOGIN_BODY" | jq -r '.token')
[ -n "$TOKEN" ] && [ "$TOKEN" != "null" ] \
  || die "failed to authenticate as $BR_ADMIN_USER"

log "Creating tenant attribute definition"
auth -X POST "$BR_HOST/api/v1/attribute-definitions" \
  -H 'Content-Type: application/json' \
  -d '{
    "key": "tenant",
    "entity_type": "user",
    "display_name": "Tenant",
    "value_type": "string",
    "allowed_values": ["acme", "globex", "stark"],
    "description": "Which customer tenant this user belongs to"
  }' >/dev/null 2>&1 || true

log "Creating users alice, bob, charlie and setting tenant attributes"
# Password must satisfy the BR admin policy: 8+ chars, upper/lower/digit/special.
for spec in "alice:Demo1234!:acme" "bob:Demo1234!:globex" "charlie:Demo1234!:stark"; do
  IFS=':' read -r USER PASS TENANT <<< "$spec"
  auth -X POST "$BR_HOST/api/v1/users" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"$USER\",\"password\":\"$PASS\"}" >/dev/null 2>&1 || true

  USER_ID=$(auth "$BR_HOST/api/v1/users" \
    | jq -r --arg u "$USER" '.data[] | select(.username==$u) | .id')
  [ -n "$USER_ID" ] && [ "$USER_ID" != "null" ] \
    || die "failed to find user $USER after create"
  auth -X PUT "$BR_HOST/api/v1/users/$USER_ID" \
    -H 'Content-Type: application/json' \
    -d "{\"attributes\":{\"tenant\":\"$TENANT\"}}" >/dev/null
done

log "Creating datasource $DATASOURCE_NAME"
DS_ID=$(auth "$BR_HOST/api/v1/datasources" \
  | jq -r --arg n "$DATASOURCE_NAME" '.data[] | select(.name==$n) | .id')
if [ -z "$DS_ID" ] || [ "$DS_ID" = "null" ]; then
  # access_mode: "open" — tables are visible to any user with datasource
  # access, and row_filter/column_mask/column_deny policies narrow the view.
  # "policy_required" would additionally require a matching column_allow
  # policy per table, which is out of scope for the demo (the guides only
  # use row filters, masks, and denies).
  DS_BODY=$(jq -n \
    --arg name "$DATASOURCE_NAME" \
    --arg host "$BR_UPSTREAM_HOST" \
    --argjson port "$BR_UPSTREAM_PORT" \
    --arg db   "$BR_UPSTREAM_DB" \
    --arg user "$BR_UPSTREAM_USER" \
    --arg pass "$BR_UPSTREAM_PASS" \
    '{
      name: $name,
      ds_type: "postgres",
      access_mode: "open",
      config: {
        host: $host, port: $port, database: $db,
        username: $user, password: $pass, sslmode: "disable"
      }
    }')
  DS_ID=$(auth -X POST "$BR_HOST/api/v1/datasources" \
    -H 'Content-Type: application/json' \
    -d "$DS_BODY" | jq -r '.id')
  [ -n "$DS_ID" ] && [ "$DS_ID" != "null" ] \
    || die "failed to create datasource $DATASOURCE_NAME"
fi
log "  datasource id: $DS_ID"

log "Saving catalog for $DATASOURCE_NAME (7 tables in public)"
SAVE_CATALOG_BODY=$(cat <<'JSON'
{
  "action": "save_catalog",
  "schemas": [
    {
      "schema_name": "public",
      "is_selected": true,
      "tables": [
        {"table_name": "organizations",   "table_type": "BASE TABLE", "is_selected": true},
        {"table_name": "customers",       "table_type": "BASE TABLE", "is_selected": true},
        {"table_name": "products",        "table_type": "BASE TABLE", "is_selected": true},
        {"table_name": "orders",          "table_type": "BASE TABLE", "is_selected": true},
        {"table_name": "order_items",     "table_type": "BASE TABLE", "is_selected": true},
        {"table_name": "payments",        "table_type": "BASE TABLE", "is_selected": true},
        {"table_name": "support_tickets", "table_type": "BASE TABLE", "is_selected": true}
      ]
    }
  ]
}
JSON
)
JOB_RESPONSE=$(auth -X POST "$BR_HOST/api/v1/datasources/$DS_ID/discover" \
  -H 'Content-Type: application/json' \
  -d "$SAVE_CATALOG_BODY")
JOB_ID=$(echo "$JOB_RESPONSE" | jq -r '.job_id // .id')
[ -n "$JOB_ID" ] && [ "$JOB_ID" != "null" ] \
  || die "failed to submit save_catalog job: $JOB_RESPONSE"

log "Polling discovery job $JOB_ID"
for _ in $(seq 1 120); do
  STATUS=$(auth "$BR_HOST/api/v1/datasources/$DS_ID/discover/$JOB_ID" \
    | jq -r '.status')
  case "$STATUS" in
    completed) log "  discovery complete"; break ;;
    failed)    die "discovery job failed" ;;
    *)         sleep 0.5 ;;
  esac
done

# --- Phase 3: policies + access -------------------------------------------
log "Phase 3 — creating policies from policies.yaml"
python3 -c "
import yaml, json, sys
with open('$SCRIPT_DIR/policies.yaml') as f:
    for p in yaml.safe_load(f)['policies']:
        print(json.dumps(p))
" | while IFS= read -r POLICY_JSON; do
  NAME=$(echo "$POLICY_JSON" | jq -r '.name')
  log "  policy: $NAME"
  echo "$POLICY_JSON" | auth -X POST "$BR_HOST/api/v1/policies" \
    -H 'Content-Type: application/json' --data-binary @- \
    >/dev/null 2>&1 || true
done

log "Granting user access to $DATASOURCE_NAME"
USER_IDS=$(auth "$BR_HOST/api/v1/users" \
  | jq -c '[.data[] | select(.username=="alice" or .username=="bob" or .username=="charlie") | .id]')
auth -X PUT "$BR_HOST/api/v1/datasources/$DS_ID/users" \
  -H 'Content-Type: application/json' \
  -d "{\"user_ids\":$USER_IDS}" >/dev/null

log "Assigning tenant-isolation, masking, and deny policies with scope=all"
ASSIGN_POLICIES=(
  tenant-isolation
  mask-ssn-partial
  hide-credit-card
  hide-product-financials
)
for POLICY_NAME in "${ASSIGN_POLICIES[@]}"; do
  POLICY_ID=$(auth "$BR_HOST/api/v1/policies" \
    | jq -r --arg n "$POLICY_NAME" '.data[] | select(.name==$n) | .id')
  if [ -z "$POLICY_ID" ] || [ "$POLICY_ID" = "null" ]; then
    log "  warning: policy $POLICY_NAME not found, skipping"
    continue
  fi
  auth -X POST "$BR_HOST/api/v1/datasources/$DS_ID/policies" \
    -H 'Content-Type: application/json' \
    -d "{\"policy_id\":\"$POLICY_ID\",\"scope\":\"all\"}" \
    >/dev/null 2>&1 || true
done

log "Setup complete. Try:"
log "  psql 'postgresql://alice:Demo1234!@127.0.0.1:5434/demo_ecommerce' \\"
log "       -c 'SELECT DISTINCT org FROM orders'   # → acme only"
