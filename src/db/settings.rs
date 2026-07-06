use anyhow::{Context, Result};
use rusqlite::params;

use super::Database;

impl Database {
	pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
		let conn = self.connection();
		let mut stmt = conn
			.prepare("SELECT value FROM settings WHERE key = ?1")
			.context("failed to prepare get_setting query")?;
		let mut rows = stmt
			.query_map(params![key], |row| row.get::<_, String>(0))
			.context("failed to query setting")?;
		match rows.next() {
			Some(row) => row.map(Some).context("failed to read setting"),
			None => Ok(None),
		}
	}

	pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
		self.connection()
			.execute(
				"INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
				params![key, value],
			)
			.context("failed to set setting")?;
		Ok(())
	}

	pub fn delete_setting(&self, key: &str) -> Result<()> {
		self.connection()
			.execute("DELETE FROM settings WHERE key = ?1", params![key])
			.context("failed to delete setting")?;
		Ok(())
	}

	pub fn get_all_settings(&self) -> Result<Vec<(String, String)>> {
		let conn = self.connection();
		let mut stmt = conn
			.prepare("SELECT key, value FROM settings ORDER BY key")
			.context("failed to prepare get_all_settings query")?;
		let rows = stmt
			.query_map([], |row| {
				Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
			})
			.context("failed to query all settings")?;
		let mut settings = Vec::new();
		for row in rows {
			settings.push(row.context("failed to read setting row")?);
		}
		Ok(settings)
	}
}
