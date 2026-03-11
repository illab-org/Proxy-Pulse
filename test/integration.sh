#!/usr/bin/env bash
# Proxy Pulse Integration Test Suite
# Usage: ./test/integration.sh [BASE_URL]
set -euo pipefail

BASE="${1:-http://127.0.0.1:8080}"
PASS=0
FAIL=0
TOKEN=""

green()  { printf '\033[32m%s\033[0m\n' "$*"; }
red()    { printf '\033[31m%s\033[0m\n' "$*"; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }

assert_status() {
    local desc="$1" method="$2" url="$3" expected="$4"
    shift 4
    local status
    status=$(curl -s -o /dev/null -w '%{http_code}' -X "$method" "$@" "${BASE}${url}")
    if [ "$status" = "$expected" ]; then
        green "  ✓ $desc (HTTP $status)"
        PASS=$((PASS + 1))
    else
        red "  ✗ $desc — expected $expected, got $status"
        FAIL=$((FAIL + 1))
    fi
}

assert_json() {
    local desc="$1" method="$2" url="$3" jq_expr="$4"
    shift 4
    local body
    body=$(curl -s -X "$method" "$@" "${BASE}${url}")
    if echo "$body" | jq -e "$jq_expr" > /dev/null 2>&1; then
        green "  ✓ $desc"
        PASS=$((PASS + 1))
    else
        red "  ✗ $desc — jq '$jq_expr' failed on: $body"
        FAIL=$((FAIL + 1))
    fi
}

echo ""
echo "═══════════════════════════════════════════"
echo "  Proxy Pulse Integration Tests"
echo "  Target: $BASE"
echo "═══════════════════════════════════════════"

# ── 1. Auth Status (needs setup, before any account exists) ──
echo ""
yellow "▸ Auth Status"
assert_json "Auth status returns needs_setup" GET "/api/v1/auth/status" '.needs_setup == true'

# ── 2. Setup account ──
yellow "▸ Account Setup"
SETUP_RESP=$(curl -s -X POST "${BASE}/api/v1/auth/setup" \
    -H 'Content-Type: application/json' \
    -d '{"username":"testadmin","password":"testpass123"}')
TOKEN=$(echo "$SETUP_RESP" | jq -r '.token // empty')
if [ -n "$TOKEN" ]; then
    green "  ✓ Account created, token received"
    PASS=$((PASS + 1))
else
    red "  ✗ Setup failed: $SETUP_RESP"
    FAIL=$((FAIL + 1))
fi

# Verify setup can't be run twice
assert_status "Setup rejects duplicate" POST "/api/v1/auth/setup" 403 \
    -H 'Content-Type: application/json' -d '{"username":"test2","password":"testpass123"}'

# ── 3. Login ──
yellow "▸ Login"
LOGIN_RESP=$(curl -s -X POST "${BASE}/api/v1/auth/login" \
    -H 'Content-Type: application/json' \
    -d '{"username":"testadmin","password":"testpass123"}')
LOGIN_TOKEN=$(echo "$LOGIN_RESP" | jq -r '.token // empty')
if [ -n "$LOGIN_TOKEN" ]; then
    green "  ✓ Login successful"
    PASS=$((PASS + 1))
    TOKEN="$LOGIN_TOKEN"
else
    red "  ✗ Login failed: $LOGIN_RESP"
    FAIL=$((FAIL + 1))
fi

AUTH=(-H "Authorization: Bearer $TOKEN")

# ── 5. Health (requires auth after setup) ──
yellow "▸ Health Endpoint"
assert_json "GET /api/v1/health returns success" GET "/api/v1/health" '.success == true' "${AUTH[@]}"
assert_json "Health includes version" GET "/api/v1/health" '.data.version != null' "${AUTH[@]}"

# ── 6. Auth-protected endpoints ──
yellow "▸ Authenticated Endpoints"
assert_json "GET /api/v1/auth/me" GET "/api/v1/auth/me" '.success == true' "${AUTH[@]}"
assert_json "GET /api/v1/auth/api-keys" GET "/api/v1/auth/api-keys" '.success == true' "${AUTH[@]}"
assert_json "GET /api/v1/auth/preferences" GET "/api/v1/auth/preferences" '.success == true' "${AUTH[@]}"

# ── 7. Proxy endpoints (may be empty but should succeed) ──
yellow "▸ Proxy API"
assert_json "GET /api/v1/proxy/all" GET "/api/v1/proxy/all" '.success == true' "${AUTH[@]}"
assert_json "GET /api/v1/proxy/stats" GET "/api/v1/proxy/stats" '.success == true' "${AUTH[@]}"
assert_json "GET /api/v1/proxy/countries" GET "/api/v1/proxy/countries" '.success == true' "${AUTH[@]}"
assert_json "GET /api/v1/proxy/top" GET "/api/v1/proxy/top" '.success == true' "${AUTH[@]}"
assert_json "GET /api/v1/proxy/json" GET "/api/v1/proxy/json" '.success == true' "${AUTH[@]}"
assert_status "GET /api/v1/proxy/txt" GET "/api/v1/proxy/txt" 200 "${AUTH[@]}"
assert_status "GET /api/v1/proxy/csv" GET "/api/v1/proxy/csv" 200 "${AUTH[@]}"

# ── 8. Admin endpoints ──
yellow "▸ Admin API"
assert_json "Admin proxy list" GET "/api/v1/admin/proxy/list" '.success == true' "${AUTH[@]}"
assert_json "Admin source list" GET "/api/v1/admin/source/list" '.success == true' "${AUTH[@]}"
assert_json "Admin checker settings" GET "/api/v1/admin/settings/checker" '.success == true' "${AUTH[@]}"
assert_json "Admin system settings" GET "/api/v1/admin/settings/system" '.success == true' "${AUTH[@]}"
assert_json "Admin update check" GET "/api/v1/admin/update/check" '.success == true' "${AUTH[@]}"
assert_json "Admin releases" GET "/api/v1/admin/update/releases" '.success == true' "${AUTH[@]}"

# ── 9. Admin import ──
yellow "▸ Admin Import"
IMPORT_RESP=$(curl -s -X POST "${BASE}/api/v1/admin/proxy/import" \
    "${AUTH[@]}" -H 'Content-Type: application/json' \
    -d '{"content":"192.168.1.1:8080\n10.0.0.1:1080","protocol_hint":"http"}')
if echo "$IMPORT_RESP" | jq -e '.success == true' > /dev/null 2>&1; then
    green "  ✓ Proxy import succeeded"
    PASS=$((PASS + 1))
else
    red "  ✗ Proxy import failed: $IMPORT_RESP"
    FAIL=$((FAIL + 1))
fi

# Verify proxies were added
assert_json "Verify imported proxies" GET "/api/v1/admin/proxy/list" '.data.total >= 2' "${AUTH[@]}"

# ── 10. Unauthorized access ──
yellow "▸ Authorization Guards"
assert_status "Admin without token" GET "/api/v1/admin/proxy/list" 401
assert_status "Proxy API without token" GET "/api/v1/proxy/all" 401

# ── 11. Pages ──
yellow "▸ Pages"
assert_status "Login page" GET "/login" 200
assert_status "Admin page requires auth" GET "/admin" 303

# ── Results ──
echo ""
echo "═══════════════════════════════════════════"
TOTAL=$((PASS + FAIL))
if [ "$FAIL" -eq 0 ]; then
    green "  All $TOTAL tests passed!"
else
    red "  $FAIL/$TOTAL tests failed"
fi
echo "═══════════════════════════════════════════"
echo ""

exit "$FAIL"
