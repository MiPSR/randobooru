use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, Row, params};

use super::{BooruCustomParameter, Database};

#[derive(Debug, Clone)]
pub struct BooruRow {
	pub id: i64,
	pub name: String,
	pub enabled: bool,
	pub embed_image: bool,
	pub max_tags: usize,
	pub supports_character: bool,
	pub page_size: u64,
	pub page_base: u64,
	pub tag_separator: String,
	pub encode_tag_separator: bool,
	pub tag_spaces_as_plus: bool,
	pub character_space_replacement: String,
	pub pace_ms: u64,
	pub description: String,
	pub count_url: Option<String>,
	pub count_path_json: String,
	pub posts_url: String,
	pub posts_path_json: String,
	pub file_url_path_json: String,
	pub source_url_path_json: String,
	pub detail_url: Option<String>,
	pub detail_id_path_json: String,
	pub detail_file_url_path_json: String,
	pub detail_source_url_path_json: String,
	pub post_url: Option<String>,
	pub headers_json: String,
	pub env_params_json: String,
	pub fetch_options_json: String,
}

const COLUMNS: &str = "id, name, enabled, embed_image, max_tags, supports_character,
	page_size, page_base, tag_separator, encode_tag_separator,
	tag_spaces_as_plus, character_space_replacement, pace_ms, description,
	count_url, count_path_json, posts_url, posts_path_json,
	file_url_path_json, source_url_path_json, detail_url,
	detail_id_path_json, detail_file_url_path_json,
	detail_source_url_path_json, post_url, headers_json,
	env_params_json, fetch_options_json";

impl Database {
	pub fn add_booru(&self, booru: &BooruRow) -> Result<i64> {
		self.validate_booru_row(booru)?;
		self.connection()
			.execute(
				"INSERT INTO boorus (name, enabled, embed_image, max_tags, supports_character,
	page_size, page_base, tag_separator,
	encode_tag_separator, tag_spaces_as_plus,
	character_space_replacement, pace_ms, description, count_url, count_path_json,
	posts_url, posts_path_json, file_url_path_json,
	source_url_path_json, detail_url, detail_id_path_json,
	detail_file_url_path_json, detail_source_url_path_json,
	post_url, headers_json, env_params_json, fetch_options_json)
	VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
	?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)",
				params![
					booru.name,
					booru.enabled as i64,
					booru.embed_image as i64,
					booru.max_tags as i64,
					booru.supports_character as i64,
					booru.page_size as i64,
					booru.page_base as i64,
					booru.tag_separator,
					booru.encode_tag_separator as i64,
					booru.tag_spaces_as_plus as i64,
					booru.character_space_replacement,
					booru.pace_ms as i64,
					booru.description,
					booru.count_url,
					booru.count_path_json,
					booru.posts_url,
					booru.posts_path_json,
					booru.file_url_path_json,
					booru.source_url_path_json,
					booru.detail_url,
					booru.detail_id_path_json,
					booru.detail_file_url_path_json,
					booru.detail_source_url_path_json,
					booru.post_url,
					booru.headers_json,
					booru.env_params_json,
					booru.fetch_options_json,
				],
			)
			.context("failed to add booru")?;
		Ok(self.connection().last_insert_rowid())
	}

	pub fn update_booru(&self, booru: &BooruRow) -> Result<()> {
		self.validate_booru_row(booru)?;
		let changed = self
			.connection()
			.execute(
				"UPDATE boorus SET
	name = ?1,
	enabled = ?2,
	embed_image = ?3,
	max_tags = ?4,
	supports_character = ?5,
	page_size = ?6,
	page_base = ?7,
	tag_separator = ?8,
	encode_tag_separator = ?9,
	tag_spaces_as_plus = ?10,
	character_space_replacement = ?11,
	description = ?12,
	count_url = ?13,
	count_path_json = ?14,
	posts_url = ?15,
	posts_path_json = ?16,
	file_url_path_json = ?17,
	source_url_path_json = ?18,
	detail_url = ?19,
	detail_id_path_json = ?20,
	detail_file_url_path_json = ?21,
	detail_source_url_path_json = ?22,
	post_url = ?23,
	headers_json = ?24,
	env_params_json = ?25,
	pace_ms = ?26,
	fetch_options_json = ?27
	WHERE id = ?28",
				params![
					booru.name,
					booru.enabled as i64,
					booru.embed_image as i64,
					booru.max_tags as i64,
					booru.supports_character as i64,
					booru.page_size as i64,
					booru.page_base as i64,
					booru.tag_separator,
					booru.encode_tag_separator as i64,
					booru.tag_spaces_as_plus as i64,
					booru.character_space_replacement,
					booru.description,
					booru.count_url,
					booru.count_path_json,
					booru.posts_url,
					booru.posts_path_json,
					booru.file_url_path_json,
					booru.source_url_path_json,
					booru.detail_url,
					booru.detail_id_path_json,
					booru.detail_file_url_path_json,
					booru.detail_source_url_path_json,
					booru.post_url,
					booru.headers_json,
					booru.env_params_json,
					booru.pace_ms as i64,
					booru.fetch_options_json,
					booru.id,
				],
			)
			.context("failed to update booru")?;

		if changed == 0 {
			bail!("booru {} not found", booru.name);
		}
		Ok(())
	}

	pub fn delete_booru(&self, name: &str) -> Result<()> {
		let changed = self
			.connection()
			.execute("DELETE FROM boorus WHERE name = ?1", params![name])
			.context("failed to delete booru")?;

		if changed == 0 {
			bail!("booru {name} not found");
		}
		Ok(())
	}

	fn validate_booru_row(&self, row: &BooruRow) -> Result<()> {
		crate::config::BooruConfig::from_row(row)?;
		anyhow::ensure!(
			row.description.chars().count() <= 100,
			"description must be at most 100 characters"
		);
		let name = crate::app::custom_command_name(&row.name);
		let description = if row.description.is_empty() {
			&row.name
		} else {
			&row.description
		};
		anyhow::ensure!(
			(1..=100).contains(&description.chars().count()),
			"effective command description must be 1..100 characters"
		);
		for other in self.get_all_boorus()? {
			anyhow::ensure!(
				other.id == row.id || crate::app::custom_command_name(&other.name) != name,
				"provider command name collision"
			);
		}
		for pattern in self.get_unique_pattern_names()? {
			anyhow::ensure!(
				crate::app::discord_name_component(&pattern) != name,
				"provider command collides with a tag command"
			);
		}
		let mut boorus = self.get_all_boorus()?;
		boorus.retain(|b| b.id != row.id);
		boorus.push(row.clone());
		for guild in self.get_validated_servers()? {
			self.validate_guild_capacity(&boorus, &self.get_server_tags(guild.guild_id)?)?;
		}
		Ok(())
	}

	pub(super) fn validate_guild_capacity(
		&self,
		boorus: &[BooruRow],
		tags: &[String],
	) -> Result<()> {
		let mut commands = boorus.iter().filter(|b| b.enabled).count() + 5;
		for tag in tags {
			let ids = self.get_booru_ids_for_pattern(tag)?;
			if boorus.iter().any(|b| b.enabled && ids.contains(&b.id)) {
				commands += 1;
			}
		}
		anyhow::ensure!(
			commands <= 100,
			"configuration would exceed Discord's 100 commands per guild"
		);
		Ok(())
	}

	pub fn set_booru_enabled(&self, name: &str, enabled: bool) -> Result<()> {
		let mut row = self
			.get_booru_by_name(name)?
			.ok_or_else(|| anyhow::anyhow!("provider {name} not found"))?;
		row.enabled = enabled;
		self.update_booru(&row)
	}

	pub fn get_booru_by_name(&self, name: &str) -> Result<Option<BooruRow>> {
		self.query_one_booru("WHERE name = ?1", params![name])
	}

	pub fn get_booru_by_id(&self, id: i64) -> Result<Option<BooruRow>> {
		self.query_one_booru("WHERE id = ?1", params![id])
	}

	pub fn get_all_boorus(&self) -> Result<Vec<BooruRow>> {
		self.query_boorus("ORDER BY name", [])
	}

	pub fn get_enabled_boorus(&self) -> Result<Vec<BooruRow>> {
		self.query_boorus("WHERE enabled = 1 ORDER BY name", [])
	}

	pub fn set_booru_custom_parameter(
		&self,
		booru_name: &str,
		key: &str,
		value: &str,
	) -> Result<()> {
		let booru = self
			.get_booru_by_name(booru_name)?
			.ok_or_else(|| anyhow::anyhow!("booru {booru_name} not found"))?;
		if key.trim().is_empty() {
			bail!("custom parameter key cannot be empty");
		}

		self.connection()
			.execute(
				"INSERT OR REPLACE INTO booru_custom_parameters (booru_id, key, value)
				 VALUES (?1, ?2, ?3)",
				params![booru.id, key, value],
			)
			.context("failed to set booru custom parameter")?;
		Ok(())
	}

	pub fn delete_booru_custom_parameter(&self, booru_name: &str, key: &str) -> Result<()> {
		let booru = self
			.get_booru_by_name(booru_name)?
			.ok_or_else(|| anyhow::anyhow!("booru {booru_name} not found"))?;
		let changed = self
			.connection()
			.execute(
				"DELETE FROM booru_custom_parameters WHERE booru_id = ?1 AND key = ?2",
				params![booru.id, key],
			)
			.context("failed to delete booru custom parameter")?;

		if changed == 0 {
			bail!("custom parameter {key} not found for {booru_name}");
		}
		Ok(())
	}

	pub fn get_booru_custom_parameters(
		&self,
		booru_name: &str,
	) -> Result<Vec<BooruCustomParameter>> {
		self.query_custom_parameters("WHERE b.name = ?1 ORDER BY p.key", params![booru_name])
	}

	pub fn get_all_booru_custom_parameters(&self) -> Result<Vec<BooruCustomParameter>> {
		self.query_custom_parameters("ORDER BY b.name, p.key", [])
	}

	fn query_one_booru(
		&self,
		suffix: &str,
		params: impl rusqlite::Params,
	) -> Result<Option<BooruRow>> {
		let sql = format!("SELECT {COLUMNS} FROM boorus {suffix}");
		self.connection()
			.prepare(&sql)
			.context("failed to prepare booru query")?
			.query_row(params, read_booru_row)
			.optional()
			.context("failed to query booru")
	}

	fn query_boorus(&self, suffix: &str, params: impl rusqlite::Params) -> Result<Vec<BooruRow>> {
		let sql = format!("SELECT {COLUMNS} FROM boorus {suffix}");
		let mut stmt = self
			.connection()
			.prepare(&sql)
			.context("failed to prepare boorus query")?;
		let rows = stmt
			.query_map(params, read_booru_row)
			.context("failed to query boorus")?;
		let mut boorus = Vec::new();
		for row in rows {
			boorus.push(row.context("failed to read booru row")?);
		}
		Ok(boorus)
	}

	fn query_custom_parameters(
		&self,
		suffix: &str,
		params: impl rusqlite::Params,
	) -> Result<Vec<BooruCustomParameter>> {
		let sql = format!(
			"SELECT b.name, p.key, p.value
			 FROM booru_custom_parameters p
			 JOIN boorus b ON b.id = p.booru_id
			 {suffix}"
		);
		let mut stmt = self
			.connection()
			.prepare(&sql)
			.context("failed to prepare booru custom parameters query")?;
		let rows = stmt
			.query_map(params, read_custom_parameter_row)
			.context("failed to query booru custom parameters")?;
		let mut parameters = Vec::new();
		for row in rows {
			parameters.push(row.context("failed to read booru custom parameter")?);
		}
		Ok(parameters)
	}
}

fn read_booru_row(row: &Row<'_>) -> rusqlite::Result<BooruRow> {
	Ok(BooruRow {
		id: row.get(0)?,
		name: row.get(1)?,
		enabled: row.get::<_, i64>(2)? != 0,
		embed_image: row.get::<_, i64>(3)? != 0,
		max_tags: row.get::<_, i64>(4)? as usize,
		supports_character: row.get::<_, i64>(5)? != 0,
		page_size: row.get::<_, i64>(6)? as u64,
		page_base: row.get::<_, i64>(7)? as u64,
		tag_separator: row.get(8)?,
		encode_tag_separator: row.get::<_, i64>(9)? != 0,
		tag_spaces_as_plus: row.get::<_, i64>(10)? != 0,
		character_space_replacement: row.get(11)?,
		pace_ms: row.get::<_, i64>(12)? as u64,
		description: row.get(13)?,
		count_url: row.get(14)?,
		count_path_json: row.get(15)?,
		posts_url: row.get(16)?,
		posts_path_json: row.get(17)?,
		file_url_path_json: row.get(18)?,
		source_url_path_json: row.get(19)?,
		detail_url: row.get(20)?,
		detail_id_path_json: row.get(21)?,
		detail_file_url_path_json: row.get(22)?,
		detail_source_url_path_json: row.get(23)?,
		post_url: row.get(24)?,
		headers_json: row.get(25)?,
		env_params_json: row.get(26)?,
		fetch_options_json: row.get(27)?,
	})
}

fn read_custom_parameter_row(row: &Row<'_>) -> rusqlite::Result<BooruCustomParameter> {
	Ok(BooruCustomParameter {
		booru_name: row.get(0)?,
		key: row.get(1)?,
		value: row.get(2)?,
	})
}
