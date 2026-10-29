# Agent specification

Rust Discord bot with SQLite-driven providers, patterns and guild commands. All
provider behavior is configured; do not introduce provider-specific Rust branches.
There is intentionally no NSFW flag or content-rating behavior.

## Runtime and deployment

- `RANDOBOORU_SECRETS_FILE` selects a **runtime** TOML file; manual default is
  `./secrets.toml`. NixOS default is `/var/lib/randobooru/secrets.toml`.
- Never import, generate or embed secret contents through Nix. Secrets are manually
  provisioned, service-user owned and mode `0600`. The state directory is `0700`.
- `RANDOBOORU_DB_PATH` selects SQLite; default is `randobooru.sqlite3`, relative to
  the working directory. NixOS exposes `dataDir`, `secretsFile` (a string) and `dbPath`.
- The flake defines and exports the package directly, including its build-input
  selection. Keep runtime secrets and databases out of build inputs.
- Startup errors exit nonzero and print useful contextual errors to stderr/systemd.
  No application-managed log files, full request URLs, headers or raw HTTP bodies.
- There is no doctor mode. Unsupported CLI arguments are rejected.
- Read secrets on every reload, initialize/migrate SQLite, clean stale rows
  transactionally, validate configuration and every replacement command set before
  making Discord command changes, then start the gateway with `GUILDS` intent.
- TLS uses native TLS/OpenSSL for both HTTP clients; never disable verification.

## Permissions

- `admin_user_id` is the sole global administrator.
- Moderators are assigned to a specific guild. NULL/global legacy moderator rows
  do not grant access; new global moderator assignment is rejected.
- Guild moderators manage only that guild's configured channels and server-tag
  whitelist. Providers, shared patterns, global settings, server validation and
  moderator assignment are administrator-only.
- The private administration panel binds opaque action IDs to a user, guild and
  expiration. Re-authorize **every** button and modal, including confirmations.
- All panel writes require confirmation. Mutation logic uses typed fields, never
  reconstructs legacy text commands, and verifies target-guild scope.
- Ordinary users need a validated server and a configured channel. Privileged users
  bypass channel admission, not channel tag/content blacklists.
- Guild moderator and administrator may request reload. This is the explicit
  exception to guild-local moderator operations.

## Commands and panel

- `/administrate`: complete private management panel, with paginated listings and
  target selectors, safe Unicode truncation, typed modals, and confirmations.
- Main categories: providers, shared patterns, server tags, channels, settings,
  servers (always available to owner), moderators, reload.
- Provider actions: inspect configuration (header values hidden), add, edit any
  field, delete, enable/disable, manage custom parameters, test configurable tags.
- Pattern actions: add/replace, delete, test against an enabled configured provider.
- Provider tests remember the last successful comma-separated tag input.
- `/reload`, `/booru-test booru:NAME tags:CSV`, `/tag-test tag:NAME` expose important
  operations without navigating the panel. Tests are ephemeral and do not alter
  history; slash tag tests honor server/channel configuration.
- Validated guilds also register `<provider>-custom`, whitelisted pattern names and
  `/art-history previous_arts:COUNT`. Empty server whitelist means no pattern commands.
- Custom commands require `tag_1`; total options are `max_tags`, or 9 when zero.
  `max_tags` is at most 25. Normalize user tag spaces according to provider config.
- Validate normalized command names, reserved names, collisions, descriptions and
  Discord command counts before registration. Bulk replacement removes stale
  commands without clearing working guild commands first.
- Register on joining a guild. Command activity checks the configurable
  `command_sync_interval_secs` (default 86400). Successful panel writes reconcile
  commands immediately. Do not register repeatedly for every initial guild event.

## Fetching and response lifecycle

- Custom and pattern commands share fetching, blacklist, retry, delivery and history.
- Independently randomize page and post order. Pattern retries reselect providers.
  Do not reuse one modulo token for page and index.
- Missing `count_url` skips counting and fetches `page_base`.
- Try eligible alternatives on a fetched page, with a bounded detail-request budget.
- Persistent per-provider pacing and a shared global API pacer cover count, list,
  detail and image requests, including concurrent commands and redirects.
- All headers are applied only to configured API origins or exact additional
  `fetch_options.trusted_image_origins`. Re-evaluate every redirect; prohibit
  HTTPS-to-HTTP downgrades. Authenticated CDN images must remain supported.
- `fetch_options.tag_paths` selects tags relative to a post or detail document.
  Generic default paths support strings, arrays and categorized objects. Normalize
  both blacklist entries and returned tags. Active blacklists fail closed when tag
  metadata cannot be inspected. Do not silently discard malformed filter JSON.
- Bound HTTP timeouts, JSON/image body sizes and concurrent jobs. Image decoding
  and compression use bounded blocking workers with decoder allocation limits.
  Reject results still above the Discord upload limit.
- Defer interactions promptly, especially before DM uploads. Once deferred, edit
  the existing response on every success/error/exhaustion path.
- Serialize final deduplication/delivery/history commit per channel, without holding
  the synchronous database mutex over network awaits. Failed deliveries must not
  consume history entries. Propagate unexpected errors to one reporting boundary.

## Database and DM references

- SQLite uses foreign keys, WAL, busy timeout and versioned transactional migrations.
- Legacy `count_url NOT NULL` is migrated without destroying provider/pattern rows.
- Very old allowlists require reconfiguration as server whitelists/channel blacklists.
- Cleanup deletes children before orphan parents and rolls back on invalid filters
  or unresolved foreign-key violations. Startup validates disabled providers too.
- Server tag names must exist as patterns. Channel blacklist entries must be in the
  guild whitelist; removing a server tag also removes its channel blacklist entries.
- Updating a channel preserves its blacklist. Adding an existing channel through
  the panel is rejected; editing prefills existing values.
- History is per channel, keyed by source link, pruned to the live configured limit.
  `/art-history` sends long results as an attachment.
- `delivered_posts` maps a random opaque ID and channel to post metadata and an
  optional `art_history` row. Foreign-key cascade expires references when history
  is pruned. References without history are capped per channel and expire after
  24 hours. IDs must never reset/rebind across reloads.
- DM buttons contain `dm:<id>`, are channel-bound, survive restarts while retained,
  and send to the clicking user. Inline images are re-fetched using current provider
  authentication; do not store attachment bytes in a long-lived RAM cache.

## Settings and reload

`src/settings.rs` is the authority for defaults and bounds. Validate before saving,
at startup, and when reading values. Settings and custom parameters are loaded live;
do not silently keep a stale startup snapshot after panel edits. Channel locale
overrides fall back to the configured application locale.

Reload stops accepting new jobs, drains jobs for up to 30 seconds, cancels unfinished
interaction work if necessary, and refuses to start a second client if cancellation
or shutdown cannot complete. Await shard-manager shutdown **while Client::start is
still running**, then join that client before starting another loop. Notifications
must not lose completion signals. SIGTERM/SIGINT follow the same shutdown lifecycle.

Unhandled errors and unknown-command bugs are logged and reported to the configured
owner once. Permission/channel/tag guards are not owner-error reports. Bound total
report length and log failures to deliver reports. Suppress mentions in generated
messages.

## Checks

Use the pinned `nix develop` environment. Run formatting, strict all-target Clippy,
tests, dependency auditing, flake evaluation and the Nix build. Database tests must
enable production foreign-key pragmas. Cover authorization/replay/scope, migrations,
cleanup rollback, persistent DM references, command validation, random-selection
independence, HTTP redirects/header scoping, filtering, body bounds, pacing and job
shutdown. Local mock servers are preferable to real provider requests in tests.
