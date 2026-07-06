use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

mod art_history;
mod boorus;
mod channels;
mod moderators;
mod patterns;
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
                count_url TEXT NOT NULL,
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
            );",
	)
	.context("failed to initialize database schema")
}
