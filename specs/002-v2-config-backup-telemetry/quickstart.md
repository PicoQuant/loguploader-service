# Quickstart — validate the v2 agent end to end

Proves the feature works: build a per-product binary, run one cycle against the real backend,
and confirm the heartbeat and a config backup landed and are retrievable.

## Prerequisites

- Rust stable toolchain with the `x86_64-pc-windows-msvc` target (build on Windows, or
  cross-compile).
- A test fleet token for the target product in `./.env`
  (`TELEMETRY_FLEET_TOKENS_LUMINOSA=...`). `.env` is gitignored.
- For the retrieval checks: `EXPECTED_ADMIN_API_KEY` in `./.env` (local testing only — never
  ships).
- Backend reachable at `https://api.picoquant.com` with the product enabled
  (`luminosa` is; `solira` needs adding to `ALLOWED_PRODUCTS` — see
  `specs/003-backend-api-support`).

## 1. Build a product binary

```
# build.rs reads PQ_PRODUCT + PQ_CHANNEL + the matching token from .env
PQ_PRODUCT=luminosa PQ_CHANNEL=beta cargo build --release
```

Expect: `target/release/pquploader-luminosa-beta.exe` (drop `PQ_CHANNEL` or set it to
`stable` for `pquploader-luminosa.exe`). A release build fails if the product or channel is
invalid or the token is empty. See `contracts/cli.md` for all 4 variant names.

Verify no secret in the tree:
```
git grep -nI "$(grep -m1 TELEMETRY_FLEET_TOKENS_LUMINOSA .env | cut -d= -f2)" ; echo "exit=$?"   # expect exit=1 (not found)
```

## 2. Run a single cycle

On a Luminosa instrument (or a box with the paths stubbed):
```
pquploader-luminosa.exe once
```

Expected: exits 0 and prints a `CycleRecord` JSON showing
- `heartbeat.ok = true`
- one `files[]` entry per discovered watched file with `action` ∈
  `sent | deduplicated | unchanged | skipped_today | blocked | retry_later`
- on first run: every present file `sent` (FR-016)

Re-run immediately:
```
pquploader-luminosa.exe once
```
Expected: heartbeat `ok` again; every file now `unchanged` (nothing changed) — proves
change detection (SC-003).

Modify a settings `*.xml` file, run `once` again → that file `sent`; run `once` a third time
without touching it → `unchanged`; touch it again the same day → `skipped_today` (the
once-per-UTC-day gate, SC-004).

Modify `PQDevice.conf`, run `once`, modify it again, run `once` → `sent` both times: the
`PQDevice` files are exempt from the daily gate (FR-011a).

## 3. Confirm it landed (backend admin queries)

```
set -a; . ./.env; set +a
API=https://api.picoquant.com ; ADMIN="$EXPECTED_ADMIN_API_KEY" ; SN=<instrument serial>

# heartbeat
curl -s -H "X-ADMIN-API-KEY: $ADMIN" \
  "$API/api/v2/admin/products/luminosa/telemetry?instrument_serial=$SN&measurement_type=agent_status&limit=1" | jq

# backups
curl -s -H "X-ADMIN-API-KEY: $ADMIN" \
  "$API/api/v2/admin/products/luminosa/backups?instrument_serial=$SN" | jq '.backups[].file_key'

# latest pqdevice_conf, byte-exact round trip
curl -s -H "X-ADMIN-API-KEY: $ADMIN" -o /tmp/dl.bin \
  "$API/api/v2/admin/products/luminosa/backups/latest?instrument_serial=$SN&file_key=pqdevice_conf" # -> get id, then .../backups/{id}/content
```

Expected: the telemetry record has `auth_kind: "fleet"`, the right `agent_version` and OS
fields; the backup list contains `pqdevice_db`, `pqdevice_conf`, and the `settings/…` /
`usersettings/…` keys; downloaded content sha256 matches what was on disk.

(The shell flow above is the same one in this repo's scratchpad `test_submission.sh`, which
already passes against `v2.2.0-beta.2`.)

## 4. Failure-path spot checks (mock server, `cargo test`)

```
cargo test
```
Covers, without touching the real backend: change detection, the UTC-day gate incl. the
midnight boundary, `401 / 413 / 422 / 5xx / connection-refused` categorisation and retry,
daily-allowance-not-consumed-on-failure, `state.json` corruption recovery, multipart format,
product/path resolution.

## 5. Install as a service (on a test machine)

```
pquploader-luminosa.exe install
sc start PQUploaderLuminosa
# ... wait one cycle interval (default 30 min, or set config.toml cycle_interval_secs=60) ...
# check Event Viewer -> Windows Logs -> Application, source "PicoQuant Luminosa LogUploader"
# check C:\ProgramData\PicoQuant\Luminosa\v2agent\cycles.log and state.json
shutdown /r /t 0                     # reboot
# ... after the machine is back, with NO interactive logon ...
sc query PQUploaderLuminosa          # STATE == RUNNING
# confirm cycles.log / state.json got a fresh entry post-reboot
pquploader-luminosa.exe uninstall    # add --purge to also drop the v2agent dir
```

Expected: service starts without a logon (auto-delayed / LocalSystem), resumes on its own
after the reboot, logs a cycle summary to the Event Log each interval, `state.json` updates,
and stop is responsive.

This step is **manual / not automated** (T040) — `windows-service` SCM integration cannot be
exercised from `cargo test`.

## 6. Fleet token rotation drill (optional)

The accept-two-values rotation (FR-025, SC-007) is documented step-by-step in `README.MD` →
**Fleet token rotation**. To rehearse it against the test backend: add a second value to
`TELEMETRY_FLEET_TOKENS_LUMINOSA` (`new,old`), rebuild, confirm `once` still succeeds, then
remove `old` from the backend and confirm a binary still carrying only `old` now gets `401`
`authentication` while the `new` build keeps working.

## Done when

- Steps 2–3 show a heartbeat and at least one config backup for the test instrument, with a
  byte-exact backup round trip.
- Re-running `once` sends nothing when nothing changed.
- `cargo test` passes.
- The token value does not appear anywhere in the git tree.
- Step 5 (manual): the service survives a reboot with no logon.
