# Contract — CLI / Service commands

The v2 binary is `pquploader-<product>.exe` (one per product). Invoked by the Windows Service
Control Manager for normal operation; the other subcommands are for install and support.

| Command | Auth/role | Behaviour | Exit |
|---|---|---|---|
| `pquploader-<product>.exe` (no args) | called by SCM | Enters the service dispatcher; runs the cycle loop until `SERVICE_CONTROL_STOP`. Not meant to be run by hand (prints guidance if started from a console, like v1). | 0 on clean stop |
| `... run` | admin (SCM) | Alias for the no-arg SCM entrypoint. | |
| `... debug` | admin console | Runs the same cycle loop in the foreground, logging to stdout + Event Log. Ctrl-C stops. For field diagnosis. | 0 |
| `... once` | admin console | Runs exactly one cycle (heartbeat + backup pass), prints a `CycleRecord` as JSON, exits. Used by `quickstart.md` and support. | 0 if cycle completed (even with per-file blocks); 1 on internal error |
| `... install` | admin | Registers the service (`PQUploader<Product>`, display name `PicoQuant <Product> Log Uploader`, start = automatic-delayed, account = LocalSystem), registers the Event Log source. Idempotent. | 0 / non-zero on failure |
| `... uninstall` | admin | Stops and removes the service and Event Log source. Leaves `C:\ProgramData\PicoQuant\<Product>\v2agent\` in place (state/logs) unless `--purge`. | 0 |
| `... version` | any | Prints `PQ_VERSION`, product, `api_base_url`, and whether a fleet token is compiled in (yes/no — never the value). | 0 |

**Notes**
- No command ever prints the fleet token, and `version` only reports presence.
- `install` / `uninstall` mirror v1's `loguploaderservice.exe install|start|stop|remove`
  semantics closely enough that `specs/001-v2-remote-upgrade`'s updater can call them.
- Service name, display name, and account are part of the migration surface — changes must go
  through `specs/001-v2-remote-upgrade` (Constitution Principle VI).
