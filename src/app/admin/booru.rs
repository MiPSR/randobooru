use crate::{config::BooruConfig, db::BooruRow};
use anyhow::{Context, Result, bail};

/// Typed configuration conversion shared by panel forms and tests.
pub(crate) fn from_json(name: &str, json: &str) -> Result<BooruRow> {
	let mut value: serde_json::Value =
		serde_json::from_str(json).context("invalid provider JSON")?;
	let object = value
		.as_object_mut()
		.context("provider JSON must be an object")?;
	let enabled = object
		.remove("enabled")
		.map(|v| v.as_bool().context("enabled must be boolean"))
		.transpose()?
		.unwrap_or(true);
	let supports_character = object
		.remove("supports_character")
		.map(|v| v.as_bool().context("supports_character must be boolean"))
		.transpose()?
		.unwrap_or(false);
	let description = object
		.remove("description")
		.map(|v| {
			v.as_str()
				.map(String::from)
				.context("description must be a string")
		})
		.transpose()?
		.unwrap_or_default();
	object.insert("name".into(), name.into());
	let config: BooruConfig =
		serde_json::from_value(value).context("invalid provider configuration")?;
	config.validate()?;
	let mut row = config.to_row()?;
	row.enabled = enabled;
	row.supports_character = supports_character;
	row.description = description;
	Ok(row)
}

pub(crate) fn edit_field(row: &mut BooruRow, field: &str, value: &str) -> Result<()> {
	fn boolean(value: &str) -> Result<bool> {
		match value.trim().to_ascii_lowercase().as_str() {
			"true" | "1" | "yes" | "on" => Ok(true),
			"false" | "0" | "no" | "off" => Ok(false),
			_ => bail!("expected a boolean"),
		}
	}
	fn json(value: &str) -> Result<String> {
		Ok(serde_json::to_string(&serde_json::from_str::<
			serde_json::Value,
		>(value)?)?)
	}
	fn optional(value: &str) -> Option<String> {
		if value.is_empty() || value.eq_ignore_ascii_case("null") {
			None
		} else {
			Some(value.into())
		}
	}
	match field {
		"name" => row.name = value.into(),
		"description" => row.description = value.into(),
		"enabled" => row.enabled = boolean(value)?,
		"embed_image" => row.embed_image = boolean(value)?,
		"supports_character" => row.supports_character = boolean(value)?,
		"max_tags" => row.max_tags = value.parse()?,
		"page_size" => row.page_size = value.parse()?,
		"page_base" => row.page_base = value.parse()?,
		"pace_ms" => row.pace_ms = value.parse()?,
		"tag_separator" => row.tag_separator = value.into(),
		"character_space_replacement" => row.character_space_replacement = value.into(),
		"encode_tag_separator" => row.encode_tag_separator = boolean(value)?,
		"tag_spaces_as_plus" => row.tag_spaces_as_plus = boolean(value)?,
		"count_url" => row.count_url = optional(value),
		"detail_url" => row.detail_url = optional(value),
		"post_url" => row.post_url = optional(value),
		"posts_url" => row.posts_url = value.into(),
		"count_path" => row.count_path_json = json(value)?,
		"posts_path" => row.posts_path_json = json(value)?,
		"file_url_path" => row.file_url_path_json = json(value)?,
		"source_url_path" => row.source_url_path_json = json(value)?,
		"detail_id_path" => row.detail_id_path_json = json(value)?,
		"detail_file_url_path" => row.detail_file_url_path_json = json(value)?,
		"detail_source_url_path" => row.detail_source_url_path_json = json(value)?,
		"headers" => row.headers_json = json(value)?,
		"env_params" => row.env_params_json = json(value)?,
		"fetch_options" => row.fetch_options_json = json(value)?,
		_ => bail!("unknown provider field {field}"),
	}
	BooruConfig::from_row(row)?;
	anyhow::ensure!(
		row.description.chars().count() <= 100,
		"description must be at most 100 characters"
	);
	Ok(())
}
