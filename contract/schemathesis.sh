#!/usr/bin/env bash
# Uji properti Schemathesis terhadap docs/api/openapi.yaml dan server lokal (LMB-BE-TEST-SCHEMATHESIS).
#
# Prasyarat: server berjalan (cargo run) dengan DB uji yang sudah di-seed (contract/seed.py), `uvx`.
# Variabel:
#   BASE_URL            default http://127.0.0.1:8080/v1
#   DEVICE_KEY          X-Device-Key sama dengan DEVICE_API_KEY_GUDANG server
#   SERVICE_KEY         X-Service-Key sama dengan SERVICE_API_KEY server
#   PIN                 PIN akun seed (default 123456)
#   MAX_EXAMPLES        contoh per operasi (default 50)
#   REPORT_DIR          direktori laporan (default contract/laporan)
# Telemetri dimatikan (schemathesis.toml + SCHEMATHESIS_TELEMETRY=off). Tidak ada data dikirim keluar.
set -uo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:8080/v1}"
DEVICE_KEY="${DEVICE_KEY:?isi DEVICE_KEY (X-Device-Key)}"
SERVICE_KEY="${SERVICE_KEY:?isi SERVICE_KEY (X-Service-Key)}"
PIN="${PIN:-123456}"
MAX_EXAMPLES="${MAX_EXAMPLES:-50}"
REPORT_DIR="${REPORT_DIR:-contract/laporan}"
SPEC="docs/api/openapi.yaml"
export SCHEMATHESIS_TELEMETRY=off

jq_token() { python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])'; }

masuk() { # akun
  curl -sf -X POST "$BASE_URL/auth/masuk" -H 'content-type: application/json' \
    -d "{\"akun\":\"$1\",\"pin\":\"$PIN\"}" | jq_token
}

TOKEN_SOPIR="$(masuk sopir-02)" || { echo "login sopir-02 gagal (seed?)" >&2; exit 2; }
TOKEN_PETANI="$(masuk PTN-007)" || { echo "login PTN-007 gagal" >&2; exit 2; }
TOKEN_KEU="$(masuk keu-01)" || { echo "login keu-01 gagal" >&2; exit 2; }
TOKEN_SHIFT="$(curl -sf -X POST "$BASE_URL/gudang/masuk" -H "X-Device-Key: $DEVICE_KEY" \
  -H 'content-type: application/json' -d "{\"operator\":\"OPR-SORTIR-1\",\"pin\":\"$PIN\"}" | jq_token)" \
  || { echo "shift gudang gagal" >&2; exit 2; }

mkdir -p "$REPORT_DIR"
status=0

jalankan() { # nama, tag..., -- header...
  local nama="$1"; shift
  local args=()
  while [ "$#" -gt 0 ]; do args+=("$1"); shift; done
  echo "=== Schemathesis: $nama ==="
  uvx schemathesis --config-file contract/schemathesis.toml run "$SPEC" \
    --url "$BASE_URL" --max-examples "$MAX_EXAMPLES" --workers 1 \
    --checks all \
    --report junit --report-dir "$REPORT_DIR/$nama" \
    "${args[@]}" || status=1
}

# Tanpa kredensial: sistem, publik, dan jalur masuk/refresh.
jalankan publik --include-tag Sistem --include-tag Publik --include-tag Auth \
  -H "X-Device-Key: $DEVICE_KEY"
# Desktop gudang: token shift + kunci perangkat.
jalankan gudang --include-tag Gudang -H "Authorization: Bearer $TOKEN_SHIFT" -H "X-Device-Key: $DEVICE_KEY"
# Mobile sopir dan petani.
jalankan sopir --include-tag Sopir -H "Authorization: Bearer $TOKEN_SOPIR"
jalankan petani --include-tag Petani -H "Authorization: Bearer $TOKEN_PETANI"
# Admin: sesi admin keuangan dan kunci layanan web.
jalankan admin-layanan --include-tag Admin -H "X-Service-Key: $SERVICE_KEY"
jalankan admin-keuangan --include-tag Admin -H "Authorization: Bearer $TOKEN_KEU"

exit "$status"
