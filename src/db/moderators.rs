use anyhow::{bail, Context, Result};
use rusqlite::params;

use super::Database;

#[derive(Debug, Clone)]
pub struct BooruCustomParameter {
	pub booru_name: String,
	pub key: String,
	pub value: String,
}

impl Database {
	pub fn add_moderator(&self, user_id: i64, guild_id: Option<i64>, added_by: i64) -> Result<()> {
		self.connection()
			.execute(
				"INSERT OR IGNORE INTO moderators (user_id, guild_id, added_by) VALUES (?1, ?2, ?3)",
				params![user_id, guild_id, added_by],
			)
			.context("failed to add moderator")?;
		Ok(())
	}

	pub fn remove_moderator(&self, user_id: i64, guild_id: Option<i64>) -> Result<()> {
		let changed = self
			.connection()
			.execute(
				"DELETE FROM moderators WHERE user_id = ?1 AND guild_id IS ?2",
				params![user_id, guild_id],
			)
			.context("failed to remove moderator")?;

		if changed == 0 {
			bail!("moderator {user_id} not found");
		}
		Ok(())
	}

	pub fn list_moderators(&self) -> Result<Vec<(i64, Option<i64>)>> {
		let conn = self.connection();
		let mut stmt = conn
			.prepare("SELECT user_id, guild_id FROM moderators ORDER BY user_id")
			.context("failed to prepare list_moderators query")?;
		let rows = stmt
			.query_map([], |row| {
				Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
			})
			.context("failed to query moderators")?;
		let mut moderators = Vec::new();
		for row in rows {
			moderators.push(row.context("failed to read moderator row")?);
		}
		Ok(moderators)
	}

	pub fn is_moderator(&self, user_id: i64, guild_id: Option<i64>) -> Result<bool> {
		let conn = self.connection();
		let mut stmt = conn
			.prepare("SELECT COUNT(*) FROM moderators WHERE user_id = ?1 AND (guild_id IS NULL OR guild_id IS ?2)")
			.context("failed to prepare is_moderator query")?;
		let count: i64 = stmt
			.query_row(params![user_id, guild_id], |row| row.get(0))
			.context("failed to query moderator check")?;

		Ok(count > 0)
	}
}
