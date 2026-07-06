use anyhow::{Context, Result};
use rusqlite::{params, Row};

use super::Database;

#[derive(Debug, Clone)]
pub struct ArtHistoryEntry {
	pub source_link: String,
	pub channel_id: i64,
	pub guild_id: Option<i64>,
	pub booru_name: Option<String>,
}

impl Database {
	pub fn register_art(
		&self,
		source_link: &str,
		channel_id: i64,
		guild_id: Option<i64>,
		booru_name: Option<&str>,
	) -> Result<bool> {
		let tx = self
			.connection()
			.unchecked_transaction()
			.context("failed to start art history transaction")?;

		let changed = tx
			.execute(
				"INSERT OR IGNORE INTO art_history (source_link, channel_id, guild_id, booru_name)
                 VALUES (?1, ?2, ?3, ?4)",
				params![source_link, channel_id, guild_id, booru_name],
			)
			.context("failed to store art history")?;

		if changed == 0 {
			return Ok(false);
		}

		let count: i64 = tx
			.query_row(
				"SELECT COUNT(*) FROM art_history WHERE channel_id = ?1",
				params![channel_id],
				|row| row.get(0),
			)
			.context("failed to count art history")?;

		if count > self.history_limit() as i64 {
			let remove = count - self.history_limit() as i64;
			tx.execute(
				"DELETE FROM art_history
                 WHERE id IN (
                     SELECT id FROM art_history
                     WHERE channel_id = ?1
                     ORDER BY id ASC
                     LIMIT ?2
                 )",
				params![channel_id, remove],
			)
			.context("failed to evict old art history")?;
		}

		tx.commit()
			.context("failed to commit art history transaction")?;

		Ok(true)
	}

	pub fn art_history_exists(&self, source_link: &str, channel_id: i64) -> Result<bool> {
		let count: i64 = self
			.connection()
			.query_row(
				"SELECT COUNT(*) FROM art_history WHERE source_link = ?1 AND channel_id = ?2",
				params![source_link, channel_id],
				|row| row.get(0),
			)
			.context("failed to check art history")?;

		Ok(count > 0)
	}

	pub fn recent_art(&self, channel_id: i64, limit: usize) -> Result<Vec<ArtHistoryEntry>> {
		let mut stmt = self
			.connection()
			.prepare(
				"SELECT source_link, channel_id, guild_id, booru_name
                 FROM art_history
                 WHERE channel_id = ?1
                 ORDER BY id DESC
                 LIMIT ?2",
			)
			.context("failed to prepare recent_art query")?;
		let rows = stmt
			.query_map(params![channel_id, limit as i64], read_art_entry)
			.context("failed to query recent art")?;
		let mut entries = Vec::new();
		for row in rows {
			entries.push(row.context("failed to read art entry")?);
		}
		Ok(entries)
	}
}

fn read_art_entry(row: &Row<'_>) -> rusqlite::Result<ArtHistoryEntry> {
	Ok(ArtHistoryEntry {
		source_link: row.get(0)?,
		channel_id: row.get(1)?,
		guild_id: row.get(2)?,
		booru_name: row.get(3)?,
	})
}
