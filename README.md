# randobooru

Database-configured Rust Discord bot. Fetch random posts, maintain per-channel
history, and manage providers and permissions through a private `/administrate`
panel. Provider-specific URLs, headers, tag paths and credentials are configuration,
not hardcoded integrations.

## Discord setup

Create an application and bot in the Discord Developer Portal. Invite the bot with
the `bot` and `applications.commands` OAuth2 scopes. Grant View Channels, Send
Messages, Embed Links and Attach Files in the channels it will use. The bot requests
the non-privileged Guilds gateway intent; it does not require Message Content.

Record the bot token, application ID and your Discord user ID. The configured user
is the **administrator everywhere**. Moderators are explicitly assigned **per guild**;
Discord's own moderator/administrator permissions do not grant bot-administration
access automatically.

## NixOS installation

Add the flake to your **own** NixOS flake (replace `host` and existing module paths):

```nix
{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.randobooru.url = "github:MiPSR/randobooru";

  outputs = { nixpkgs, randobooru, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        randobooru.nixosModules.default
        { services.randobooru.enable = true; }
      ];
    };
  };
}
```

The defaults are:

- dedicated user/group: `randobooru`;
- working directory: `/var/lib/randobooru`, mode `0700`;
- runtime secrets: `/var/lib/randobooru/secrets.toml`;
- database: `/var/lib/randobooru/randobooru.sqlite3`.

Rebuild your configuration to create the service account and data directory:

```sh
sudo nixos-rebuild switch --flake /path/to/your/config#host
```

On first installation, the service will report a missing runtime secrets file until
you provision it. This is intentional: **Nix never writes or imports the secret
contents**. Create the file at runtime using your existing provisioning mechanism,
or interactively:

```sh
sudo install -d -m 0700 -o randobooru -g randobooru /var/lib/randobooru
sudo install -m 0600 -o randobooru -g randobooru /dev/null /var/lib/randobooru/secrets.toml
sudoedit /var/lib/randobooru/secrets.toml
```

Do not run that `install /dev/null` command over an existing secrets file: it
replaces its contents. Required TOML:

```toml
discord_token = "your-bot-token"
admin_user_id = 123456789012345678
discord_application_id = 123456789012345678
```

Then start the service:

```sh
sudo systemctl restart randobooru
systemctl status randobooru --no-pager
```

Errors go to stderr and systemd's journal, not to application-managed log files.
For more than the status summary, use `journalctl -u randobooru -b`. Startup failures
exit nonzero; systemd retries with `Restart=on-failure`. Requests are logged by type,
not by exact URL/header. Secrets parse failures do not print the credential lines.

### Custom runtime locations

```nix
services.randobooru = {
  enable = true;
  dataDir = "/var/lib/randobooru";
  secretsFile = "/var/lib/randobooru/secrets.toml"; # STRING, runtime path only
  dbPath = "randobooru.sqlite3"; # relative to dataDir, or an absolute writable path
};
```

Never use `builtins.readFile`, `secretsText`, or a Nix path literal containing your
credentials. `secretsText` has been removed. Provision the runtime file with mode
`0600` and service-user ownership. `/var` provides persistence, not encryption:
protect backups too. With the module's filesystem sandbox, writable database paths
must remain inside `dataDir` (or require an explicit `ReadWritePaths` override).

## Manual installation / development

From a checkout:

```sh
nix build
```

The executable is `result/bin/randobooru`. The package is defined directly in
`flake.nix`, which includes only source/build inputs, never the runtime secrets,
database or `target` directory. Use `nix build`, not the legacy `nix-build` command.
When developing in a Git checkout, add new source files to Git before evaluating
the Git flake: Nix does not include untracked files. Never add runtime secrets or
databases to Git to work around a build error.

Run from a separate state directory, not from the checkout:

```sh
install -d -m 0700 "$HOME/.local/state/randobooru"
# Create secrets.toml there and chmod it to 0600 before running.
cd "$HOME/.local/state/randobooru"
/absolute/path/to/checkout/result/bin/randobooru
```

Paths can instead be set with `RANDOBOORU_SECRETS_FILE` and `RANDOBOORU_DB_PATH`.
There are no CLI flags or doctor mode. For a direct flake run, first enter the state
directory containing your runtime secrets, then use `nix run github:MiPSR/randobooru`.

Development uses the pinned flake toolchain:

```sh
nix develop
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo audit
```

HTTP and Discord clients use native TLS/OpenSSL. The NixOS service supplies a CA
bundle automatically. On other deployments, ensure system CA certificates exist;
`SSL_CERT_FILE` can select a bundle. No insecure TLS bypass is provided.

## First-time configuration

1. Run `/administrate` in your server as the configured administrator.
2. Under **Servers**, validate the server. This page is always available to you.
3. Add a **Booru** using its name, description, URLs JSON, paths JSON and options
   JSON. The form includes editable examples; replace `example.org` with your API.
4. Set any provider custom parameters and reference them through `env_params`.
5. Use **Test** to supply comma-separated tags. The last successful provider-test
   tags are remembered. You can also test defined patterns from **Tag patterns**.
6. Add channels, define shared tag patterns, then enable those names under **Server
   tags**. No tag-generated commands are enabled by an empty server whitelist.
7. Optionally assign guild moderators. They can manage only their guild's channels
   and enabled server tags. Shared providers, shared patterns, settings, server
   validation and moderator assignment remain administrator-only.

The panel is ephemeral, paginated and user/guild-bound. Writes require confirmation.
Category listings hide credential values. Provider inspection hides header values;
avoid putting literal credentials in URLs—use placeholders/custom parameters.
Configuration changes take effect immediately and trigger command reconciliation.
Old guild commands are removed by bulk replacement; the working set is not cleared
before replacement validation.

### Commands

- `/administrate`: full management panel.
- `/reload`: administrator or guild moderator; drain active jobs, await old gateway
  shutdown, reload runtime secrets/configuration, then reconnect.
- `/booru-test booru:NAME tags:tag_one,tag_two`: administrator-only private preview.
- `/tag-test tag:NAME`: administrator/guild-moderator private preview of an enabled
  server pattern, honoring that channel's blacklist.
- `/<provider>-custom tag_1:...`: custom tags for an enabled provider.
- `/<pattern>`: random enabled provider for a pattern whitelisted in this server.
- `/art-history previous_arts:COUNT`: recent links for the current channel; long
  results are attached as text.

Provider and pattern tests do not add entries to art history. Normal deliveries
are recorded only after successful Discord delivery. Guild commands are initialized
when the bot joins; normal command activity also checks whether reconciliation is
due. `command_sync_interval_secs` defaults to **86400** (24 hours).

### Authenticated image/CDN downloads

Configured headers are reused automatically on API origins. For a different image
host that also needs those headers, edit the provider's `fetch_options` field:

```json
{
  "trusted_image_origins": ["https://images.example.org"],
  "tag_paths": [["tags"]]
}
```

Origins are exact scheme/host/port values, not wildcards or paths. Each redirect
is rechecked and HTTPS-to-HTTP redirects are rejected. Other image hosts are fetched
without provider headers. This preserves account-only downloads without sending
credentials to every arbitrary image URL.

`tag_paths` are paths relative to each post, or the detail document when a detail
endpoint is configured. Strings, arrays and nested category objects are supported.
Default paths cover common generic tag fields. If a blacklist is active and the
configured paths cannot be inspected, the post is rejected rather than bypassing
filtering. Blacklists are case-normalized.

## Settings

| Key | Default |
|---|---|
| `app_lang` | `en` (`fr` also available for existing translated messages) |
| `api_rate_pace_ms` | `0` |
| `discord_api_pace_ms` | `0` |
| `booru_fetch_retry_limit` | `3` |
| `booru_source_link_history_limit` | `1000` per channel |
| `booru_tag_blacklist` | empty comma-separated list |
| `command_sync_interval_secs` | `86400` |
| `http_timeout_secs` | `30` per request |
| `max_concurrent_jobs` | `4` |

Per-provider `pace_ms` additionally limits that provider across concurrent requests.
Downloads are bounded (8 MiB JSON, 40 MiB images); oversized attachments are
compressed off the async runtime and rejected if still over 10 MiB. The bot does
not retain image bytes in memory between interactions.

**Send to DM** buttons use persistent random references scoped to the source
channel. For posts in art history, references survive reload/restart and disappear
when that history entry is pruned. Posts without a history source use a bounded
per-channel fallback retained for up to 24 hours. Inline DM images are downloaded
again, so deleted provider files or expired authenticated URLs may no longer work.

## Backups and upgrades

Stop the service before copying the state directory, or use SQLite's online backup
API; do not copy only the database file while ignoring its WAL. The database holds
provider custom parameters as well as history, so treat its backup as sensitive.

Schema changes are versioned and transactional. Legacy `count_url NOT NULL`
databases migrate to optional count URLs. Very old channel allowlists require
explicit reconfiguration as server whitelists/channel blacklists; no attempt is
made to guess old permission intent. Malformed filter JSON aborts startup with a
contextual error rather than silently removing filtering. This version is intended
to be configured from scratch before first deployment.

Run the development checks above and `nix flake check` / `nix build` locally.
These checks do not substitute for testing your actual Discord server permissions
and each provider's API configuration.
