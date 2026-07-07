use anyhow::{Context, Result};
use rusqlite::params;

use super::Database;

impl Database {
	pub fn add_server_tag(&self, guild_id: i64, tag_name: &str) -> Result<()> {
		self.connection()
			.execute(
				"INSERT OR IGNORE INTO server_tag_whitelist (guild_id, tag_name) VALUES (?1, ?2)",
				params![guild_id, tag_name],
			)
			.context("failed to add server tag")?;
		Ok(())
	}

	pub fn remove_server_tag(&self, guild_id: i64, tag_name: &str) -> Result<()> {
		self.connection()
			.execute(
				"DELETE FROM server_tag_whitelist WHERE guild_id = ?1 AND tag_name = ?2",
				params![guild_id, tag_name],
			)
			.context("failed to remove server tag")?;
		Ok(())
	}

	pub fn get_server_tags(&self, guild_id: i64) -> Result<Vec<String>> {
		let conn = self.connection();
		let mut stmt = conn
			.prepare(
				"SELECT tag_name FROM server_tag_whitelist
                 WHERE guild_id = ?1
                 ORDER BY tag_name",
			)
			.context("failed to prepare get_server_tags query")?;
		let rows = stmt
			.query_map(params![guild_id], |row| row.get::<_, String>(0))
			.context("failed to query server tags")?;
		let mut names = Vec::new();
		for row in rows {
			names.push(row.context("failed to read server tag")?);
		}
		Ok(names)
	}

	pub fn count_server_tag_whitelist(&self) -> Result<usize> {
		let count: i64 = self
			.connection()
			.query_row("SELECT COUNT(*) FROM server_tag_whitelist", [], |row| {
				row.get(0)
			})
			.context("failed to count server tag whitelist")?;
		Ok(count as usize)
	}
}
