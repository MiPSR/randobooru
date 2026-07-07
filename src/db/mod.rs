use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::config::BooruConfig;

mod art_history;
mod boorus;
mod channels;
mod moderators;
mod patterns;
mod server_tags;
mod servers;
mod settings;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use art_history::ArtHistoryEntry;
pub use boorus::BooruRow;
pub use channels::ChannelConfig;
#[allow(unused_imports)]
pub use moderators::BooruCustomParameter;
#[allow(unused_imports)]
pub use patterns::{TagPattern, TagPatternEntry};
#[allow(unused_imports)]
pub use servers::Server;

pub struct Database {
	conn: Connection,
	history_limit: usize,
}

impl Database {
	pub fn open(path: impl AsRef<Path>, history_limit: usize) -> Result<Self> {
		let path = path.as_ref();
		let conn = Connection::open(path)
			.with_context(|| format!("failed to open database {}", path.display()))?;

		conn.execute_batch(
			"PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;",
		)
		.context("failed to set pragmas")?;

		initialize_schema(&conn)?;

		Ok(Self {
			conn,
			history_limit,
		})
	}

	pub(crate) fn connection(&self) -> &Connection {
		&self.conn
	}

	pub(crate) fn history_limit(&self) -> usize {
		self.history_limit
	}

	pub fn check_and_clean(&self) -> Result<usize> {
		let mut cleaned = 0usize;
		let conn = self.connection();

		cleaned += conn
			.execute(
				"DELETE FROM tag_pattern_entries
				 WHERE pattern_id NOT IN (SELECT id FROM tag_patterns)",
				[],
			)
			.context("failed to clean orphan tag pattern entries")?;
		cleaned += conn
			.execute(
				"DELETE FROM tag_patterns
				 WHERE booru_id NOT IN (SELECT id FROM boorus)",
				[],
			)
			.context("failed to clean orphan tag patterns")?;
		cleaned += conn
			.execute(
				"DELETE FROM booru_custom_parameters
				 WHERE booru_id NOT IN (SELECT id FROM boorus)",
				[],
			)
			.context("failed to clean orphan booru custom parameters")?;
		cleaned += conn
			.execute(
				"DELETE FROM channel_patterns
				 WHERE (guild_id, channel_id) NOT IN (SELECT guild_id, channel_id FROM channels)",
				[],
			)
			.context("failed to clean orphan channel patterns")?;
		cleaned += conn
			.execute(
				"DELETE FROM channel_patterns
				 WHERE (guild_id, pattern_name) NOT IN
				       (SELECT guild_id, tag_name FROM server_tag_whitelist)",
				[],
			)
			.context("failed to clean channel blacklist names not in server whitelist")?;
		cleaned += conn
			.execute(
				"DELETE FROM server_tag_whitelist
				 WHERE guild_id NOT IN (SELECT guild_id FROM servers)",
				[],
			)
			.context("failed to clean orphan server tag whitelist entries")?;
		cleaned += conn
			.execute(
				"DELETE FROM channels
				 WHERE guild_id NOT IN (SELECT guild_id FROM servers)",
				[],
			)
			.context("failed to clean orphan channels")?;
		cleaned += conn
			.execute(
				"UPDATE channels SET banned_tags = '[]'
				 WHERE json_valid(banned_tags) = 0 OR json_type(banned_tags) != 'array'",
				[],
			)
			.context("failed to clean invalid channel banned_tags")?;

		for booru in self.get_all_boorus()? {
			BooruConfig::from_row(&booru)
				.with_context(|| format!("invalid booru config for {}", booru.name))?;
		}

		Ok(cleaned)
	}
}

fn initialize_schema(conn: &Connection) -> Result<()> {
	conn.execute_batch(
		"CREATE TABLE IF NOT EXISTS servers (
                guild_id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                validated INTEGER NOT NULL DEFAULT 0,
                interaction_channels TEXT NOT NULL DEFAULT '[]',
                joined_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS moderators (
                user_id INTEGER NOT NULL,
                guild_id INTEGER,
                added_by INTEGER NOT NULL,
                added_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (user_id, guild_id)
            );

            CREATE TABLE IF NOT EXISTS boorus (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                enabled INTEGER NOT NULL DEFAULT 1,
                embed_image INTEGER NOT NULL DEFAULT 0,
                max_tags INTEGER NOT NULL DEFAULT 0,
                supports_character INTEGER NOT NULL DEFAULT 0,
                page_size INTEGER NOT NULL,
                page_base INTEGER NOT NULL DEFAULT 0,
                tag_separator TEXT NOT NULL DEFAULT ' ',
                encode_tag_separator INTEGER NOT NULL DEFAULT 0,
                tag_spaces_as_plus INTEGER NOT NULL DEFAULT 0,
                character_space_replacement TEXT NOT NULL DEFAULT '+',
                description TEXT NOT NULL DEFAULT '',
                count_url TEXT,
                count_path_json TEXT NOT NULL DEFAULT '[]',
                posts_url TEXT NOT NULL,
                posts_path_json TEXT NOT NULL DEFAULT '[]',
                file_url_path_json TEXT NOT NULL DEFAULT '[]',
                source_url_path_json TEXT NOT NULL DEFAULT '[]',
                detail_url TEXT,
                detail_id_path_json TEXT NOT NULL DEFAULT '[]',
                detail_file_url_path_json TEXT NOT NULL DEFAULT '[]',
                detail_source_url_path_json TEXT NOT NULL DEFAULT '[]',
                post_url TEXT,
                headers_json TEXT NOT NULL DEFAULT '{}',
                env_params_json TEXT NOT NULL DEFAULT '[]'
            );

            CREATE TABLE IF NOT EXISTS tag_patterns (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                booru_id INTEGER NOT NULL,
                FOREIGN KEY (booru_id) REFERENCES boorus(id) ON DELETE CASCADE,
                UNIQUE(name, booru_id)
            );

            CREATE TABLE IF NOT EXISTS booru_custom_parameters (
                booru_id INTEGER NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                PRIMARY KEY (booru_id, key),
                FOREIGN KEY (booru_id) REFERENCES boorus(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS tag_pattern_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pattern_id INTEGER NOT NULL,
                tag TEXT NOT NULL,
                is_excluded INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (pattern_id) REFERENCES tag_patterns(id) ON DELETE CASCADE,
                UNIQUE(pattern_id, tag)
            );

            CREATE TABLE IF NOT EXISTS art_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_link TEXT NOT NULL,
                channel_id INTEGER NOT NULL,
                guild_id INTEGER,
                booru_name TEXT,
                sent_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(source_link, channel_id)
            );

            CREATE INDEX IF NOT EXISTS idx_art_history_channel
                ON art_history(channel_id, id DESC);

            CREATE INDEX IF NOT EXISTS idx_tag_patterns_name
                ON tag_patterns(name);

            CREATE TABLE IF NOT EXISTS channels (
                guild_id INTEGER NOT NULL,
                channel_id INTEGER NOT NULL,
                nsfw INTEGER NOT NULL DEFAULT 0,
                language TEXT,
                banned_tags TEXT NOT NULL DEFAULT '[]',
                PRIMARY KEY (guild_id, channel_id),
                FOREIGN KEY (guild_id) REFERENCES servers(guild_id)
            );

            CREATE TABLE IF NOT EXISTS channel_patterns (
                guild_id INTEGER NOT NULL,
                channel_id INTEGER NOT NULL,
                pattern_name TEXT NOT NULL,
                PRIMARY KEY (guild_id, channel_id, pattern_name),
                FOREIGN KEY (guild_id, channel_id) REFERENCES channels(guild_id, channel_id)
            );

            CREATE TABLE IF NOT EXISTS server_tag_whitelist (
                guild_id INTEGER NOT NULL,
                tag_name TEXT NOT NULL,
                PRIMARY KEY (guild_id, tag_name),
                FOREIGN KEY (guild_id) REFERENCES servers(guild_id)
            );",
	)
	.context("failed to initialize database schema")?;

	let _ =
		conn.execute_batch("ALTER TABLE boorus ADD COLUMN description TEXT NOT NULL DEFAULT '';");

	Ok(())
}
