use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension, Row};

use super::Database;

#[derive(Debug, Clone)]
pub struct Server {
	pub guild_id: i64,
	pub name: String,
	pub validated: bool,
	pub interaction_channels: Vec<i64>,
}

impl Database {
	pub fn add_server(&self, guild_id: i64, name: &str) -> Result<()> {
		self.connection()
			.execute(
				"INSERT OR IGNORE INTO servers (guild_id, name) VALUES (?1, ?2)",
				params![guild_id, name],
			)
			.context("failed to add server")?;
		Ok(())
	}

	pub fn get_server(&self, guild_id: i64) -> Result<Option<Server>> {
		self.connection()
			.query_row(
				"SELECT guild_id, name, validated, interaction_channels FROM servers WHERE guild_id = ?1",
				params![guild_id],
				read_server_row,
			)
			.optional()
			.context("failed to query server")
	}

	pub fn set_validated(&self, guild_id: i64, validated: bool) -> Result<()> {
		let changed = self
			.connection()
			.execute(
				"UPDATE servers SET validated = ?1 WHERE guild_id = ?2",
				params![validated as i64, guild_id],
			)
			.context("failed to set server validated")?;

		if changed == 0 {
			bail!("server {guild_id} not found");
		}
		Ok(())
	}

	pub fn get_validated_servers(&self) -> Result<Vec<Server>> {
		self.query_servers("WHERE validated = 1", [])
	}

	pub fn get_all_servers(&self) -> Result<Vec<Server>> {
		self.query_servers("ORDER BY guild_id", [])
	}

	pub fn set_interaction_channels(&self, guild_id: i64, channels: &[i64]) -> Result<()> {
		let json = serde_json::to_string(channels).context("failed to serialize channels")?;
		let changed = self
			.connection()
			.execute(
				"UPDATE servers SET interaction_channels = ?1 WHERE guild_id = ?2",
				params![json, guild_id],
			)
			.context("failed to set interaction channels")?;

		if changed == 0 {
			bail!("server {guild_id} not found");
		}
		Ok(())
	}

	fn query_servers(&self, suffix: &str, params: impl rusqlite::Params) -> Result<Vec<Server>> {
		let sql =
			format!("SELECT guild_id, name, validated, interaction_channels FROM servers {suffix}");
		let mut stmt = self
			.connection()
			.prepare(&sql)
			.context("failed to prepare server query")?;
		let rows = stmt
			.query_map(params, read_server_row)
			.context("failed to query servers")?;
		let mut servers = Vec::new();
		for row in rows {
			servers.push(row.context("failed to read server row")?);
		}
		Ok(servers)
	}
}

fn read_server_row(row: &Row<'_>) -> rusqlite::Result<Server> {
	Ok(Server {
		guild_id: row.get(0)?,
		name: row.get(1)?,
		validated: row.get::<_, i64>(2)? != 0,
		interaction_channels: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
	})
}

#[cfg(test)]
pub(super) fn open_test_db() -> Database {
	use rusqlite::Connection;

	use super::initialize_schema;

	let conn = Connection::open_in_memory().expect("in-memory db");
	initialize_schema(&conn).expect("schema");
	Database {
		conn,
		history_limit: 3,
	}
}
