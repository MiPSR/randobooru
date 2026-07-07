# Agent Specification

## Purpose
Rust Discord bot. The bot registers slash commands from a SQLite database, fetches random posts from configured booru APIs, and replies with source links and the direct image URL.

Provider behavior must come from database config only. No provider-specific Rust logic.

## Runtime
Startup order:
1. Load `secrets.toml`.
2. Open SQLite database at `randobooru.sqlite3`.
3. Log `Checking the DB.`.
4. Clean stale runtime rows that cannot make sense: orphan tag pattern entries, orphan tag patterns, orphan booru custom parameters, channel patterns referencing an unconfigured channel, channel blacklist entries not in the server's whitelist, server tag whitelist entries for missing guilds, orphan channels, invalid channel `banned_tags` JSON.
5. Validate all booru configs. Invalid booru config aborts startup.
6. Load settings from database.
7. Validate required config values.
8. Build per-guild slash-command sets from database state and built-in commands (see Command Registration).
9. Replace each connected server's slash commands.
10. Start the gateway.

Command execution (tag-generated commands):
1. Look up the tag pattern in the database by command name.
2. Require the tag to be whitelisted on the server, unless the user is admin or moderator for that guild. If the tag is blacklisted in the current channel, reply with `channel_tag_blocked`.
3. Pick a random booru from enabled boorus that have this pattern.
4. Build tags from pattern entries (included and excluded).
5. Apply global tag blacklist, pattern-excluded tags, and channel `banned_tags`.
6. Fetch post count (skipped when the booru has no `count_url`; posts are fetched from `page_base` instead).
7. Pick a random page.
8. Fetch posts.
9. Pick a random post.
10. Check art history (per channel, see Art History).
11. On history hit, retry from step 3.
12. Resolve source URL and direct file URL.
13. Register in art history if the post has a source URL.
14. Reply with source links and the direct file URL. Attach a "Send to DM" button (see DM Button).

Command execution (custom commands):
1. Pick the booru from the command name suffix.
2. Collect user-provided tags.
3. Apply global tag blacklist and channel `banned_tags`.
4. Steps 6-14 same as tag-generated commands.

Shared behavior uses the same implementation path. Custom and tag-generated commands share fetch, retry, blacklist, art history, and response logic. Only booru selection and tag source differ.

When the bot receives a slash command whose name is not registered, the user sees `command_not_registered` and a `BUG` DM is sent to the bot owner.

## Command Registration
Commands are registered **per guild** through `set_commands`. Each guild's command set is:
- `<booru>-custom` for every enabled booru (description from `boorus.description` or the booru name).
- One slash command per unique tag name that appears in the guild's `server_tag_whitelist` AND has at least one enabled booru configured for it.
- Built-in commands: `art-history`, `administrate`.

Empty `server_tag_whitelist` for a validated guild means no tag-generated slash commands are registered. Custom and built-in commands are always available.

## Database
All runtime state and config stored in `randobooru.sqlite3`.

### servers
- `guild_id INTEGER PRIMARY KEY` -- Discord server ID.
- `name TEXT NOT NULL` -- Server name.
- `validated INTEGER NOT NULL DEFAULT 0` -- Approval status.
- `interaction_channels TEXT NOT NULL DEFAULT '[]'` -- Legacy JSON array of channel IDs.
- `joined_at TEXT NOT NULL DEFAULT (datetime('now'))`

### settings
- `key TEXT PRIMARY KEY`
- `value TEXT NOT NULL`

Supported keys and defaults:
- `app_lang = en`
- `api_rate_pace_ms = 0`
- `booru_fetch_retry_limit = 3`
- `booru_source_link_history_limit = 1000`
- `booru_tag_blacklist = ""`

### moderators
- `user_id INTEGER NOT NULL` -- Discord user ID.
- `guild_id INTEGER` -- Server ID. NULL means global.
- `added_by INTEGER NOT NULL` -- Admin who added.
- `added_at TEXT NOT NULL DEFAULT (datetime('now'))`
- `PRIMARY KEY (user_id, guild_id)`

### boorus
All booru config fields stored as columns. JSON paths and maps stored as JSON strings. See `BooruConfig` struct in `config.rs` for field list.

Additional columns:
- `description TEXT NOT NULL DEFAULT ''` -- Custom slash command description. Empty uses the booru name.

### tag_patterns
- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `name TEXT NOT NULL` -- Pattern name. Becomes a slash command **only when the server's whitelist includes it**.
- `booru_id INTEGER NOT NULL REFERENCES boorus(id) ON DELETE CASCADE`
- `UNIQUE(name, booru_id)`

### tag_pattern_entries
- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `pattern_id INTEGER NOT NULL REFERENCES tag_patterns(id) ON DELETE CASCADE`
- `tag TEXT NOT NULL`
- `is_excluded INTEGER NOT NULL DEFAULT 0` -- 0 included, 1 excluded.
- `UNIQUE(pattern_id, tag)`

### booru_custom_parameters
- `booru_id INTEGER NOT NULL`
- `key TEXT NOT NULL`
- `value TEXT NOT NULL`
- `PRIMARY KEY (booru_id, key)`
- `FOREIGN KEY (booru_id) REFERENCES boorus(id) ON DELETE CASCADE`

### art_history
- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `source_link TEXT NOT NULL`
- `channel_id INTEGER NOT NULL`
- `guild_id INTEGER`
- `booru_name TEXT`
- `sent_at TEXT NOT NULL DEFAULT (datetime('now'))`
- `UNIQUE(source_link, channel_id)`

Pruning per channel against `booru_source_link_history_limit`.

### channels
- `guild_id INTEGER NOT NULL`
- `channel_id INTEGER NOT NULL`
- `nsfw INTEGER NOT NULL DEFAULT 0` -- Legacy column. Runtime ignores. New writes always 0.
- `language TEXT` -- Optional locale override.
- `banned_tags TEXT NOT NULL DEFAULT '[]'` -- JSON array of additional blocked tags.
- `PRIMARY KEY (guild_id, channel_id)`
- `FOREIGN KEY (guild_id) REFERENCES servers(guild_id)`

### channel_patterns (channel blacklist)
- `guild_id INTEGER NOT NULL`
- `channel_id INTEGER NOT NULL`
- `pattern_name TEXT NOT NULL` -- Tag name (must be in the server's `server_tag_whitelist` for the same guild) that is **blocked** in this channel.
- `PRIMARY KEY (guild_id, channel_id, pattern_name)`
- `FOREIGN KEY (guild_id, channel_id) REFERENCES channels(guild_id, channel_id)`

Cleanup removes rows whose `pattern_name` is not in `server_tag_whitelist`.

### server_tag_whitelist
- `guild_id INTEGER NOT NULL`
- `tag_name TEXT NOT NULL` -- Slash command name. Must match a configured tag pattern.
- `PRIMARY KEY (guild_id, tag_name)`
- `FOREIGN KEY (guild_id) REFERENCES servers(guild_id)`

Empty whitelist = no tag-generated commands registered for the guild.

## Booru Config
Boorus stored in database. See `BooruConfig` struct in `config.rs` for full field list.

Optional fields:
- `count_url`: count API URL template. When omitted, the count step is skipped and posts are fetched from `page_base` only. Suitable for boorus without a separate count endpoint.
- `post_url`: post URL template. When omitted, no booru post source link appears in responses.
- `detail_url`: detail API URL template. When omitted, `file_url_path` and `source_url_path` are read from the post list response.
- `description`: custom description for the booru's slash command. Empty by default. When empty, the booru name is used as the command description.

`env_params` is a list of objects with fields:
- `placeholder`: template placeholder name.
- `env`: key to look up in the runtime values map.
- `source`: `custom`, `settings`, or `secrets`.

Supported URL template placeholders:
- `{tags}`: encoded final tag query.
- `{page}`: randomly selected page number.
- `{limit}`: configured `page_size`.
- `{id}`: detail or post ID when available.
- Extra placeholders declared in `env_params`.

`source = "custom"` uses booru-specific values from `booru_custom_parameters`, managed with `booru parameter` subcommands. `source = "settings"` uses database settings. `source = "secrets"` uses runtime secrets such as `discord_token`.

`supports_character` is stored but has no runtime behavior.

There is intentionally no nsfw flag, filter, or behavior anywhere in the codebase.

## Tags
A tag entry groups included and excluded booru tags for a booru. Each unique tag name with at least one enabled booru can be whitelisted per guild via `server_tag_whitelist`. Multiple boorus with the same tag name produce one shared slash command on that guild.

Rules:
- Tag-generated commands accept no options.
- Tag-generated commands pick a random booru from all enabled boorus with the tag entry name.
- On retry, tag-generated commands re-randomize which booru to use.
- A tag must be in the server's whitelist to be registered as a slash command.
- `tags add` creates or replaces a tag pattern. Existing entries for the pattern and booru are deleted and replaced.

## Custom Commands
Each enabled booru registers `<name>-custom` on every validated guild.

Rules:
- Accept user-provided tags (`tag_1` through `tag_N`).
- `tag_1` is required.
- `N` is `max_tags`, or 9 when `max_tags = 0`.
- Fixed booru selection from command name.
- Spaces in user tags are replaced with the booru's `character_space_replacement`.
- Command description comes from the booru's `description` field. When empty, the booru name is used.

## Admin Commands
`/administrate` slash command. Displays an embed-based administration panel with button navigation.

Rules:
- Available to admin and moderators.
- Takes no options.
- Server validation and unvalidation restricted to admin only.
- Validate button appears in the main menu only when the current server is unvalidated.
- Uses Discord embeds with button components for navigation and actions.
- Edit, delete, toggle, parameter, server-tag, channel, and setting actions select the concerned item before applying the action.
- Destructive actions require confirmation via Yes/No buttons before execution.
- Data input uses Discord modals with text input fields.
- Add/edit modals split input into separate fields. The bot merges separate fields into the final stored config when needed.
- Each category page shows current state and action buttons.

Category pages:

### Tags
Lists tag patterns grouped by booru with included and excluded booru tags. The pool of configured tag names.

Actions: Add Tags (modal), Remove Tags (select pattern, confirmation).

### Server Tags
Per-guild whitelist that controls which tag-generated slash commands are registered. Empty = no tag commands.

Actions: Add Tag (modal with the tag name), Remove Tag (select, confirmation).

### Boorus
Lists all boorus with enabled status, description, page_size, and max_tags.

Actions: Add (modal with separated name, description, URLs JSON, paths JSON, options JSON), Edit (select booru, modal with field and value), Delete (select booru, confirmation), Toggle enabled (select booru, confirmation), Parameters (select booru, modal with action, key, value).

### Channels
Lists configured channels per server with language, banned tags, and **blacklisted** tag names.

Actions: Add Channel (modal), Remove Channel (select channel, confirmation), Set Config (select channel, modal with language and banned tags), Tags (select channel, modal with add/remove/list of blacklisted tag names).

### Settings
Lists settings stored in the database.

Actions: Add Setting (modal), Edit Setting (select setting, prefilled modal), Delete Setting (select setting, confirmation).

### Validate Discord
Lists validated and pending servers. Only visible when unvalidated servers exist.

Actions: Validate Server (admin only, modal), Unvalidate Server (admin only, modal).

Button interaction custom_id format:
- `ac:<category>` -- Navigate to category page.
- `aa:<category>:<action>` -- Trigger action (modal or confirmation).
- `as:<category>:<action>[:<id>...]` -- Select a target item for the next action.
- `ay:<category>:<action>[:<id>...]` -- Confirm destructive action.
- `an` -- Cancel and return to main menu.
- `am:<category>:<action>[:<id>...]` -- Modal submission handler.
- `dm:<cache_id>` -- DM button on image responses.

## Reload
Reload is triggered via a button in the `/administrate` panel (custom_id `aa:reload`). Available to admin and moderators.

Rules:
- Restarts the bot without process restart.
- Waits for active jobs to finish (up to 30 seconds).
- Shuts down the gateway in a spawned task to avoid deadlocking the interaction handler.
- Waits for the old client to fully shut down before starting the new one.
- Drops the `Client` and re-enters the main loop.
- The new loop reopens the database, rebuilds state, re-registers commands, and opens a fresh gateway.
- In-flight commands receive `reload_toml_in_progress` during restart.

### Startup sequence
**First boot:**
```
Bot loading → [initialization] → Bot loaded
```

**Reload:**
```
Bot reloading → [old client shutdown] → Unloaded everything → Bot reloaded → [new bot loading] → Bot loaded
```

The old client must fully shut down before the new client starts to prevent gateway conflicts.

## Server Validation
Rules:
- Server rows auto-added via `guild_create`.
- No admin `server add` command.
- Admin validates via the administration panel's Validate Discord page.
- Unvalidated servers reject commands except `/administrate`.
- Admin and moderators can access `/administrate` in unvalidated servers.
- Normal users are restricted to configured channels.
- Mods and admin bypass channel restrictions.
- `servers.interaction_channels` is legacy; access checks use the `channels` table.

## Channel Config
Rules:
- `language` overrides default locale when matching an available locale. Unmatched values fall back to `app_lang`.
- `banned_tags` applied as additional blacklist for all commands.
- `channel_patterns` stores the tag-generated command names **blacklisted** in this channel. Empty = nothing blacklisted.
- A user invoking a blacklisted tag in that channel sees `channel_tag_blocked`.
- Custom commands and built-in commands are not filtered by `channel_patterns`.

## Art History
Rules:
- Tracks sent source links per channel.
- Only checked and registered when a post has a source URL.
- Posts without a source URL skip duplicate avoidance.
- Prunes oldest entries per channel against `booru_source_link_history_limit`.
- `/art-history <count>` returns recent links for the current channel.
- Long responses sent as attachment.

## DM Button
Image responses include a "Send to DM" button below the post.

Rules:
- Button is present on every image response (link and embed_image boorus).
- Click sends the same post to the requesting user's DMs.
- Inline images are sent as attachments in the DM.
- Link images send the formatted text with source links and image URL.
- Post data cached in memory keyed by a short ID encoded in the button custom_id.
- Cache entries expire after 1 hour.
- Cleanup task runs every 10 minutes to remove expired entries.
- If DMs are closed, the user sees an ephemeral error message.
- Button custom_id format: `dm:<cache_id>`.

## Error Reporting
Unhandled errors and explicit bug reports are sent as a Discord DM to the bot administrator.

Rules:
- Recipient is `secrets.toml` `admin_user_id` (the same user that already controls admin-level actions). No new key is required.
- The DM body includes the severity label (`ERROR` or `BUG`), the interaction context, a short summary, and the error text in a code block.
- Reported severities:
  - `ERROR`: any unhandled error returned from `handle_command` or `handle_interaction`.
  - `BUG`: an unknown slash command reaches the bot (e.g. tag removed from the server whitelist but a stale interaction arrives). The user still sees `command_not_registered` text.
- User-facing guard messages (`server_not_validated`, `channel_not_allowed`, `channel_tag_blocked`, `tag_not_registered`) are NOT reported to the admin.
