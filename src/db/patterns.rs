use anyhow::{bail, Context, Result};
use rusqlite::{params, Row};

use super::Database;

#[derive(Debug, Clone)]
pub struct TagPattern {
	pub id: i64,
	pub name: String,
	pub booru_id: i64,
}

#[derive(Debug, Clone)]
pub struct TagPatternEntry {
	pub id: i64,
	pub pattern_id: i64,
	pub tag: String,
	pub is_excluded: bool,
}

impl Database {
	pub fn add_tag_pattern(
		&self,
		name: &str,
		booru_name: &str,
		included_tags: &[String],
		excluded_tags: &[String],
	) -> Result<()> {
		let booru = self
			.get_booru_by_name(booru_name)?
			.ok_or_else(|| anyhow::anyhow!("booru {booru_name} not found"))?;

		let tx = self
			.connection()
			.unchecked_transaction()
			.context("failed to start transaction")?;

		tx.execute(
			"INSERT OR IGNORE INTO tag_patterns (name, booru_id) VALUES (?1, ?2)",
			params![name, booru.id],
		)
		.context("failed to insert tag pattern")?;

		let pattern_id: i64 = tx
			.query_row(
				"SELECT id FROM tag_patterns WHERE name = ?1 AND booru_id = ?2",
				params![name, booru.id],
				|row| row.get(0),
			)
			.context("failed to get pattern id")?;

		tx.execute(
			"DELETE FROM tag_pattern_entries WHERE pattern_id = ?1",
			params![pattern_id],
		)
		.context("failed to clear old entries")?;

		for tag in included_tags {
			if tag.trim().is_empty() {
				continue;
			}
			tx.execute(
				"INSERT INTO tag_pattern_entries (pattern_id, tag, is_excluded) VALUES (?1, ?2, 0)",
				params![pattern_id, tag.trim()],
			)
			.context("failed to insert included tag")?;
		}

		for tag in excluded_tags {
			if tag.trim().is_empty() {
				continue;
			}
			tx.execute(
				"INSERT INTO tag_pattern_entries (pattern_id, tag, is_excluded) VALUES (?1, ?2, 1)",
				params![pattern_id, tag.trim()],
			)
			.context("failed to insert excluded tag")?;
		}

		tx.commit().context("failed to commit tag pattern")?;
		Ok(())
	}

	pub fn delete_tag_pattern(&self, name: &str, booru_name: Option<&str>) -> Result<()> {
		let tx = self
			.connection()
			.unchecked_transaction()
			.context("failed to start transaction")?;

		let changed = if let Some(booru_name) = booru_name {
            tx.execute(
                "DELETE FROM tag_patterns WHERE name = ?1 AND booru_id = (SELECT id FROM boorus WHERE name = ?2)",
                params![name, booru_name],
            )
        } else {
            tx.execute("DELETE FROM tag_patterns WHERE name = ?1", params![name])
        }
        .context("failed to delete pattern")?;

		if changed == 0 {
			bail!("tag pattern {name} not found");
		}

		tx.commit().context("failed to commit delete")?;
		Ok(())
	}

	pub fn get_tag_patterns(&self, name: Option<&str>) -> Result<Vec<TagPattern>> {
		let conn = self.connection();
		if let Some(name) = name {
			let mut stmt = conn
				.prepare(
					"SELECT p.id, p.name, p.booru_id
                     FROM tag_patterns p
                     JOIN boorus b ON b.id = p.booru_id
                     WHERE p.name = ?1
                     ORDER BY b.name",
				)
				.context("failed to prepare get_tag_patterns query")?;
			let rows = stmt
				.query_map(params![name], read_tag_pattern)
				.context("failed to query tag patterns")?;
			return collect(rows);
		}

		let mut stmt = conn
			.prepare(
				"SELECT p.id, p.name, p.booru_id
                 FROM tag_patterns p
                 ORDER BY p.name",
			)
			.context("failed to prepare get_tag_patterns query")?;
		let rows = stmt
			.query_map([], read_tag_pattern)
			.context("failed to query tag patterns")?;
		collect(rows)
	}

	pub fn get_pattern_entries(&self, pattern_id: i64) -> Result<Vec<TagPatternEntry>> {
		let mut stmt = self
			.connection()
			.prepare(
				"SELECT id, pattern_id, tag, is_excluded
                 FROM tag_pattern_entries
                 WHERE pattern_id = ?1
                 ORDER BY tag",
			)
			.context("failed to prepare get_pattern_entries query")?;
		let rows = stmt
			.query_map(params![pattern_id], read_tag_pattern_entry)
			.context("failed to query pattern entries")?;
		let mut entries = Vec::new();
		for row in rows {
			entries.push(row.context("failed to read entry row")?);
		}
		Ok(entries)
	}

	pub fn get_booru_ids_for_pattern(&self, name: &str) -> Result<Vec<i64>> {
		let mut stmt = self
			.connection()
			.prepare("SELECT booru_id FROM tag_patterns WHERE name = ?1")
			.context("failed to prepare get_booru_ids_for_pattern query")?;
		let rows = stmt
			.query_map(params![name], |row| row.get::<_, i64>(0))
			.context("failed to query booru ids")?;
		let mut ids = Vec::new();
		for row in rows {
			ids.push(row.context("failed to read booru id")?);
		}
		Ok(ids)
	}

	pub fn get_unique_pattern_names(&self) -> Result<Vec<String>> {
		let mut stmt = self
			.connection()
			.prepare("SELECT DISTINCT name FROM tag_patterns ORDER BY name")
			.context("failed to prepare get_unique_pattern_names query")?;
		let rows = stmt
			.query_map([], |row| row.get::<_, String>(0))
			.context("failed to query pattern names")?;
		let mut names = Vec::new();
		for row in rows {
			names.push(row.context("failed to read pattern name")?);
		}
		Ok(names)
	}

	pub fn count_enabled_pattern_commands(&self) -> Result<usize> {
		let count: i64 = self
			.connection()
			.query_row(
				"SELECT COUNT(DISTINCT p.name)
				 FROM tag_patterns p
				 JOIN boorus b ON b.id = p.booru_id
				 WHERE b.enabled = 1",
				[],
				|row| row.get(0),
			)
			.context("failed to count enabled pattern commands")?;
		Ok(count as usize)
	}
}

fn read_tag_pattern(row: &Row<'_>) -> rusqlite::Result<TagPattern> {
	Ok(TagPattern {
		id: row.get(0)?,
		name: row.get(1)?,
		booru_id: row.get(2)?,
	})
}

fn read_tag_pattern_entry(row: &Row<'_>) -> rusqlite::Result<TagPatternEntry> {
	Ok(TagPatternEntry {
		id: row.get(0)?,
		pattern_id: row.get(1)?,
		tag: row.get(2)?,
		is_excluded: row.get::<_, i64>(3)? != 0,
	})
}

fn collect(
	rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<TagPattern>>,
) -> Result<Vec<TagPattern>> {
	let mut patterns = Vec::new();
	for row in rows {
		patterns.push(row.context("failed to read pattern row")?);
	}
	Ok(patterns)
}
