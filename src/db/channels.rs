use anyhow::{Context, Result};
use rusqlite::{params, Row};

use super::Database;

#[derive(Debug, Clone)]
pub struct ChannelConfig {
	pub guild_id: i64,
	pub channel_id: i64,
	pub language: Option<String>,
	pub banned_tags: Vec<String>,
}

impl Database {
	pub fn set_channel_config(&self, config: &ChannelConfig) -> Result<()> {
		self.connection()
			.execute(
				"INSERT OR REPLACE INTO channels (guild_id, channel_id, nsfw, language, banned_tags)
                 VALUES (?1, ?2, 0, ?3, ?4)",
				params![
					config.guild_id,
					config.channel_id,
					config.language,
					serde_json::to_string(&config.banned_tags).unwrap_or_default(),
				],
			)
			.context("failed to set channel config")?;
		Ok(())
	}

	pub fn get_channel_config(
		&self,
		guild_id: i64,
		channel_id: i64,
	) -> Result<Option<ChannelConfig>> {
		let conn = self.connection();
		let mut stmt = conn
			.prepare(
				"SELECT guild_id, channel_id, language, banned_tags
                 FROM channels WHERE guild_id = ?1 AND channel_id = ?2",
			)
			.context("failed to prepare get_channel_config query")?;
		let mut rows = stmt
			.query_map(params![guild_id, channel_id], read_channel_row)
			.context("failed to query channel config")?;
		match rows.next() {
			Some(row) => row.map(Some).context("failed to read channel config"),
			None => Ok(None),
		}
	}

	pub fn get_guild_channels(&self, guild_id: i64) -> Result<Vec<ChannelConfig>> {
		let conn = self.connection();
		let mut stmt = conn
			.prepare(
				"SELECT guild_id, channel_id, language, banned_tags
                 FROM channels WHERE guild_id = ?1 ORDER BY channel_id",
			)
			.context("failed to prepare get_guild_channels query")?;
		let rows = stmt
			.query_map(params![guild_id], read_channel_row)
			.context("failed to query guild channels")?;
		let mut channels = Vec::new();
		for row in rows {
			channels.push(row.context("failed to read channel config")?);
		}
		Ok(channels)
	}

	pub fn count_channels(&self) -> Result<usize> {
		let count: i64 = self
			.connection()
			.query_row("SELECT COUNT(*) FROM channels", [], |row| row.get(0))
			.context("failed to count channels")?;
		Ok(count as usize)
	}

	pub fn has_channel(&self, guild_id: i64, channel_id: i64) -> Result<bool> {
		let count: i64 = self
			.connection()
			.query_row(
				"SELECT COUNT(*) FROM channels WHERE guild_id = ?1 AND channel_id = ?2",
				params![guild_id, channel_id],
				|row| row.get(0),
			)
			.context("failed to check channel")?;
		Ok(count > 0)
	}

	pub fn guild_has_channels(&self, guild_id: i64) -> Result<bool> {
		let count: i64 = self
			.connection()
			.query_row(
				"SELECT COUNT(*) FROM channels WHERE guild_id = ?1",
				params![guild_id],
				|row| row.get(0),
			)
			.context("failed to check guild channels")?;
		Ok(count > 0)
	}

	pub fn add_channel_pattern(
		&self,
		guild_id: i64,
		channel_id: i64,
		pattern_name: &str,
	) -> Result<()> {
		self.connection()
			.execute(
				"INSERT OR IGNORE INTO channel_patterns (guild_id, channel_id, pattern_name)
                 VALUES (?1, ?2, ?3)",
				params![guild_id, channel_id, pattern_name],
			)
			.context("failed to add channel pattern")?;
		Ok(())
	}

	pub fn remove_channel_pattern(
		&self,
		guild_id: i64,
		channel_id: i64,
		pattern_name: &str,
	) -> Result<()> {
		self.connection()
			.execute(
				"DELETE FROM channel_patterns
                 WHERE guild_id = ?1 AND channel_id = ?2 AND pattern_name = ?3",
				params![guild_id, channel_id, pattern_name],
			)
			.context("failed to remove channel pattern")?;
		Ok(())
	}

	pub fn get_channel_patterns(&self, guild_id: i64, channel_id: i64) -> Result<Vec<String>> {
		let conn = self.connection();
		let mut stmt = conn
			.prepare(
				"SELECT pattern_name FROM channel_patterns
                 WHERE guild_id = ?1 AND channel_id = ?2
                 ORDER BY pattern_name",
			)
			.context("failed to prepare get_channel_patterns query")?;
		let rows = stmt
			.query_map(params![guild_id, channel_id], |row| row.get::<_, String>(0))
			.context("failed to query channel patterns")?;
		let mut names = Vec::new();
		for row in rows {
			names.push(row.context("failed to read pattern name")?);
		}
		Ok(names)
	}
}

fn read_channel_row(row: &Row<'_>) -> rusqlite::Result<ChannelConfig> {
	Ok(ChannelConfig {
		guild_id: row.get(0)?,
		channel_id: row.get(1)?,
		language: row.get(2)?,
		banned_tags: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
	})
}
