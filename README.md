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
```sh
nix-build
```
Single static binary in `result/bin/randobooru`. Deployed to a Debian LXC on Proxmox.
## Usage
All management goes through the `/administrate` slash command.
Run `/administrate action:help` for available actions.
