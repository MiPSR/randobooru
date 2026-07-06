# Agent Specification
## Purpose
Rust Discord bot.
The bot registers slash commands from a SQLite database. Each command fetches a random post from a configured booru API and replies with source links and the direct image URL.
Provider behavior must come from database config only. No provider-specific Rust logic.
## Runtime
Startup order:
1. Load `secrets.toml`.
2. Open SQLite database at `randobooru.sqlite3`.
3. Load settings from database.
4. Validate required config values.
5. Build server slash-command sets from database state and built-in commands.
6. Replace each connected server's slash commands (see Command Registration).
7. Run Discord bot.
Command execution (pattern commands):
1. Look up tag pattern in database.
2. Pick random booru from enabled boorus with this pattern.
3. Build tags from pattern entries (included and excluded).
4. Apply global tag blacklist and channel `banned_tags`.
5. Fetch post count.
6. Pick random page.
7. Fetch posts.
8. Pick random post.
9. Check art history (per channel, see Art History).
10. On history hit, retry from step 2.
11. Resolve source URL and direct file URL.
12. Register in art history if post has source URL.
13. Reply with source links and the direct file URL.
Command execution (custom commands):
1. Pick booru from command name suffix.
2. Collect user-provided tags.
3. Apply global tag blacklist and channel `banned_tags`.
4. Steps 5-13 same as pattern commands.
Shared behavior uses the same implementation path. Custom and pattern commands share fetch, retry, blacklist, art history, and response logic. Only booru selection and tag source differ.
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
All booru config fields stored as columns. JSON paths and maps stored as JSON strings.
See `BooruConfig` struct in config.rs for field list.
### tag_patterns
- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `name TEXT NOT NULL` -- Pattern name. Becomes a slash command.
- `booru_id INTEGER NOT NULL REFERENCES boorus(id) ON DELETE CASCADE`
- `UNIQUE(name, booru_id)`
### tag_pattern_entries
- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `pattern_id INTEGER NOT NULL REFERENCES tag_patterns(id) ON DELETE CASCADE`
- `tag TEXT NOT NULL`
- `is_excluded INTEGER NOT NULL DEFAULT 0` -- 0 included, 1 excluded.
- `UNIQUE(pattern_id, tag)`
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
### channel_patterns
- `guild_id INTEGER NOT NULL`
- `channel_id INTEGER NOT NULL`
- `pattern_name TEXT NOT NULL`
- `PRIMARY KEY (guild_id, channel_id, pattern_name)`
## Booru Config
Boorus stored in database. See `BooruConfig` struct in config.rs for full field list.
Optional fields:
- `post_url`: post URL template. When omitted, no booru post source link appears in responses.
- `detail_url`: detail API URL template. When omitted, `file_url_path` and `source_url_path` are read from the post list response.
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
## Tag Patterns
A pattern groups included and excluded tags for a booru. Each unique pattern name with at least one enabled booru registers as a slash command.
Rules:
- Pattern commands accept no options.
- Pattern commands pick a random booru from all enabled boorus with the pattern.
- On retry, pattern commands re-randomize which booru to use.
- `tags add` creates or replaces a tag pattern. Existing entries for the pattern and booru are deleted and replaced.
## Custom Commands
Each enabled booru registers `<name>-custom`.
Rules:
- Accept user-provided tags (`tag_1` through `tag_N`).
- `tag_1` is required.
- `N` is `max_tags`, or 9 when `max_tags = 0`.
- Fixed booru selection from command name.
- Spaces in user tags are replaced with the booru's `character_space_replacement`.
## Admin Commands
`/administrate` slash command. Takes one `action` string argument.
Rules:
- Only the admin user (from `secrets.toml admin_user_id`) can use it.
- Subcommands parsed from action string.
Subcommands:
### help
Returns usage text.
### booru
- `list` -- List all boorus.
- `add <name> <json>` -- Add booru from JSON object with booru config fields. Missing `enabled` defaults to true.
- `import <path>` -- Import from TOML file.
- `<name> edit <field> <value>` -- Change one field. JSON-valued fields must be supplied as JSON. Use `null` for optional fields.
- `<name> delete` -- Delete.
- `<name> enable` -- Enable.
- `<name> disable` -- Disable.
- `<name> parameter set <key> <value>` -- Set custom parameter.
- `<name> parameter delete <key>` -- Delete custom parameter.
- `<name> parameter list` -- List custom parameters.
- `<name> tag-pattern add <pattern> <included> [excluded]` -- Add tag pattern.
- `<name> tag-pattern remove <pattern>` -- Remove tag pattern.
- `<name> tag-pattern list` -- List tag patterns.
`max_tags = 0` means no known tag limit. Custom commands expose up to 9 tag options when `max_tags = 0`.
### tags
- `list` -- List pattern names.
- `<name> add <booru> <included> [excluded]` -- Create or replace pattern.
- `<name> delete [booru]` -- Delete pattern. Without booru, deletes all entries with that name.
### moderator
- `list` -- List moderators.
- `<user_id> add [guild_id]` -- Add.
- `<user_id> remove [guild_id]` -- Remove.
### server
- `[guild_id] validate` -- Validate. Defaults to current server.
- `[guild_id] unvalidate` -- Unvalidate. Defaults to current server.
- `list` -- List servers.
- `[guild_id] channel [channel_id] add|remove` -- Manage interaction channels. Defaults to current channel.
- `[guild_id] channel [channel_id] set [language] [banned_tags]` -- Set channel config.
- `[guild_id] channel [channel_id] list` -- List channels.
- `[guild_id] channel [channel_id] patterns add|remove|list [name]` -- Manage channel patterns.
- `validated` -- List validated servers.
### channel (shortcut)
- `<channel_id> add|remove|set|list ...` -- Shortcut using current server.
- `<channel_id> patterns add|remove|list [name]` -- Shortcut using current server.
### patterns (shortcut)
- `add|remove|list [name]` -- Shortcut using current server and channel.
### setting
- `list` -- List all settings.
- `<key> get` -- Get value.
- `<key> set <value>` -- Set value.
- `<key> delete` -- Delete.
## Moderation Commands
`/reload` slash command. Available to admin and moderators.
Rules:
- Restarts bot without process restart.
- Waits for active jobs to finish (up to 30 seconds).
- Closes gateway, drops `Client`, re-enters main loop.
- New loop reopens database, rebuilds state, re-registers commands, opens fresh gateway.
- In-flight commands receive `reload_toml_in_progress` during restart.
## Server Validation
Rules:
- Server rows auto-added via `guild_create`.
- No admin `server add` command.
- Admin validates via `/administrate action:server validate <guild_id>`.
- Unvalidated servers reject commands except `/administrate`.
- Only admin can validate/unvalidate.
- Without `channels` rows, all channels allowed.
- With `channels` rows, normal users restricted to configured channels.
- Mods and admin bypass channel restrictions.
- `servers.interaction_channels` is legacy; access checks use `channels` table.
## Channel Config
Rules:
- `language` overrides default locale when matching an available locale. Unmatched values fall back to `app_lang`.
- `banned_tags` applied as additional blacklist for all commands.
- `channel_patterns` restricts which pattern commands run in a channel.
- No assigned patterns means all pattern commands allowed.
- Custom commands not filtered by `channel_patterns`.
- Built-in commands bypass `channel_patterns`.
## Art History
Rules:
- Tracks sent source links per channel.
- Only checked and registered when post has a source URL.
- Posts without source URL skip duplicate avoidance.
- Prunes oldest entries per channel against `booru_source_link_history_limit`.
- `/art-history <count>` returns recent links for current channel.
- Long responses sent as attachment.
## Response Format
```text
[source (<booru>)](<post-url>) | [source (<host>)](<source-url>) | compressed
image-url
```
Rules:
- Source links are Markdown links with angle-bracket-wrapped URLs.
- Booru source link appears only when post has a post URL.
- Host source link appears only when post has upstream source URL.
- `compressed` appears only for recompressed inline images.
- Image URL on its own line.
- `embed_image` boorus download and upload directly.
- Images over 10 MiB recompressed to JPEG.
## Command Registration
Registration uses database state.
The bot lists connected servers, clears global commands, clears each server command list, then replaces each server command list.
Validated servers receive:
- `<name>-custom` for each enabled booru.
- `<pattern_name>` for each unique tag pattern with at least one enabled booru.
- `art-history`, `reload`, `administrate`.
Unvalidated servers receive:
- `administrate`.
Registration runs at startup and after `/reload`. Database mutations update immediately but do not hot-register command changes. Use `/reload` or restart to publish.
Rules:
- Pattern names use `discord_name_component` sanitization.
- Custom command names use `<sanitized_booru>-custom`.
- Registration runs only after database is opened.
- Every connected server receives command replacement.
- Same command builder for validated and unvalidated servers.
## I18n
Locale files in `locales/` as TOML. Compiled into binary. Validated at build time.
Build script requires every locale file to contain exactly the same keys. Keys sorted at build time for binary search at runtime. Build fails on missing key, unknown key, invalid placeholder, or invalid TOML.
Channel `language` values that do not match any available locale fall back to `app_lang`.
Required keys:
- `administrate_action_description`
- `administrate_command_description`
- `administrate_help`
- `admin_only`
- `art_history_attachment_filename`
- `art_history_command_description`
- `art_history_error`
- `art_history_no_links`
- `art_history_option_description`
- `art_history_showing_all`
- `art_history_showing_count`
- `channel_not_allowed`
- `channel_patterns_empty`
- `could_not_find_image`
- `custom_command_description`
- `custom_command_no_tags`
- `custom_tag_option_description`
- `pattern_command_description`
- `reload_toml_already_in_progress`
- `reload_toml_command_description`
- `reload_toml_finished`
- `reload_toml_in_progress`
- `reload_toml_waiting`
- `required_tag_option_description`
- `server_not_validated`
Placeholder keys:
- `art_history_error`: `{error}`
- `art_history_showing_all`: `{requested}`, `{shown}`
- `art_history_showing_count`: `{shown}`
- `could_not_find_image`: `{error}`
- `custom_command_description`: `{booru}`
- `pattern_command_description`: `{pattern}`
## Errors
Startup aborts on:
- missing or invalid `secrets.toml`
- missing or invalid secret value
- unsupported active locale
- database open failure
Runtime failures:
- unvalidated server
- restricted channel mismatch
- failed count, posts, or detail request
- JSON path not found
- empty post result
- missing file URL
- response value type mismatch
- HTTP status rejected
- invalid JSON response
Rules:
- Do not silently skip invalid configs.
- Do not expose credentials in logs.
- User-facing failures should be short and localized.
## Logging
CLI only. Writes to stderr.
Rules:
- Fixed line formats only.
- Same formatter for every runtime line.
- No `RUST_LOG`.
- No file logging.
- No secrets in logs.
- No other CLI output.
Formats:
```text
<timestamp> <server> <channel> <user> <command>
Booru random step: <timestamp> <step> <random number result>
Final selection (kept): <timestamp> <booru source link> <external source link> <picture link>
Final selection (kept + compressed): <timestamp> <booru source link> <external source link> <picture link> Compressed in <compression time>
Final selection (not kept, retrying): <timestamp> <fail reason> <retry count>
<timestamp> <server> <channel> <output_type>
<timestamp> Found <servers> and <validated> and <channels> and <commands>.
<timestamp> Commands cleaned.
<timestamp> Commands pushed.
<timestamp> Server ready.
<timestamp> <error> <errortype>
```
Normalization:
- Missing values use `-`.
- Whitespace inside values becomes `_`.
- Output type values are normalized lowercase words.
- Timestamps are UTC ISO 8601 with milliseconds.
## Constraints
- Rust code must stay provider-independent.
- API behavior must come from config.
- Credentials must stay in `secrets.toml`.
- `secrets.toml` is a required project file.
- Configs stored in database, not embedded in binary.
- Supported languages defined by locale files, not hardcoded.
- Do not downgrade dependency versions.
- Release binary must be statically linked and Debian-compatible.
- Build verification: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `nix-build`.
## Implementation Consistency
Rules:
- Similar behavior uses the same method, helper, data path, and registration path.
- Differences in state represented as parameters or data, not separate systems.
- No separate implementations for validated vs unvalidated servers.
- No separate implementations for built-in vs generated command registration.
- No separate implementations for pattern vs custom command fetching, retrying, blacklist, art history, or response.
- No separate storage paths for the same config.
- No separate CLI output paths.
- New method only when behavior is structurally different.
### Code Style
- Tabs for indentation. No spaces.
- `rustfmt.toml` enforces `hard_tabs = true` and `tab_spaces = 4`.
- Nix files use tabs.
- TOML files use flat key-value layout with no indentation.
## Documentation Format Rules
### Style
- Direct statements.
- Short sections and paragraphs.
- No narrative prose, motivational text, or filler transitions.
- No explanations of intent unless required for implementation.
- No empty lines anywhere in AGENT.md and README.md. No empty lines before or after headings, around lists, around code blocks, or between paragraphs.
### Section Structure
- One `#` title.
- `##` for main sections.
- `###` only when needed.
- Group related rules. Separate unrelated rules.
- Error behavior only in `Errors`.
- Logging behavior only in `Logging`.
### Lists
- Bullets for rules, fields, variables, constraints.
- Numbered lists only for ordered steps.
- No ASCII tables.
### Code Blocks
- Fenced blocks only for paths, commands, TOML, and placeholder shapes.
- No code blocks for prose.
- No generated block IDs or metadata.
### Specific Values
Hardcoded values allowed only for:
- Fixed filenames, folder names, command names, JSON field names, placeholder names.
- Required build commands and binary path.
Not allowed:
- Language list, provider names as logic, provider defaults in Rust, absolute paths, credentials, config IDs.
### Paths
- Database path fixed at `randobooru.sqlite3`.
- Secrets path fixed at `secrets.toml`.
- Locale path fixed at `locales/`.
- Examples use placeholders for variable parts.
- No absolute paths.
### Runtime Config
- `secrets.toml` contains `discord_token`, `admin_user_id`, `discord_application_id`.
- All other settings in database `settings` table.
- Credentials stay in `secrets.toml`.
- No real values in examples.
### Error Documentation
All error behavior belongs in `Errors`.
### Generic Logic
- Provider behavior from config.
- Language support from locale files.
- Config from database.
- No provider-specific or language-specific branches in Rust.
- Same behavior uses same implementation path.
### Examples
Use placeholder names (`<name>`, `<booru>`, `<command>`, `<lang>`, `<placeholder>`). No real providers, tokens, channel IDs, or guild IDs.
### Editing Rules
- Preserve existing requirements unless explicitly changed.
- Keep wording short and implementation-ready.
- Remove duplicates. Move misplaced rules to correct sections.
- Do not add provider-specific or hardcoded language assumptions.
- Do not replace placeholders with concrete values.
- Do not convert lists to tables.
- Do not add large diagrams or ASCII trees.
- Do not add prose summaries.
