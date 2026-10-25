# randobooru
Rust Discord bot. Fetches random posts from configurable booru APIs through database-driven slash commands.
- Supports custom API tokens per booru.
- Compresses images over 10 MiB.
- Returns booru and original source links.
- Live command editing.
- Per-channel command management.
- Multi-server support.
## Setup
Create `secrets.toml` in the working directory:
```toml
discord_token = "<token>"
admin_user_id = <user_id>
discord_application_id = <application_id>
```
## Build & Deployment

### NixOS (recommended)
This repository is a Nix flake. On a NixOS machine:

```sh
# run directly
nix run github:<owner>/randobooru

# or add to your flake
# inputs.randobooru.url = "github:<owner>/randobooru";
```

Enable the provided NixOS module:

```nix
services.randobooru = {
  enable = true;
  secretsText = ''
    discord_token = "<token>"
    admin_user_id = <user_id>
    discord_application_id = <application_id>
  '';
};
```

Secrets are written to `/var/lib/randobooru/secrets.toml` (mode 0600, owned by the `randobooru` user). The database lives at `/var/lib/randobooru/randobooru.sqlite3`. Back up that directory to preserve state.

### Manual
```sh
nix-build
```
Single binary in `result/bin/randobooru`. For manual runs, launch it from the directory containing `secrets.toml`; the database is created as `randobooru.sqlite3` in that same directory. When deployed via the NixOS module, both files live under `/var/lib/randobooru/`.
## Usage
All management goes through the `/administrate` slash command.
Run `/administrate action:help` for available actions.
