use anyhow::{bail, Context, Result};

use crate::{config::BooruConfig, db::BooruRow};

use super::Handler;

pub(crate) async fn run(handler: &Handler, rest: &str) -> String {
	let parts: Vec<&str> = rest.splitn(2, ' ').collect();
	let sub = parts[0].trim().to_ascii_lowercase();
	let args = parts.get(1).copied().unwrap_or("").trim();

	match sub.as_str() {
		"" => usage().to_string(),
		"list" => list(handler).await,
		"show" => show(handler, args).await,
		"add" => add(handler, args).await,
		"edit" => edit(handler, args).await,
		"delete" => delete(handler, args).await,
		"enable" => enable(handler, args).await,
		"disable" => disable(handler, args).await,
		"parameter" | "parameters" | "param" | "params" => {
			super::parameters::run(handler, args).await
		}
		"tag-pattern" | "tag-patterns" | "tags" => super::tag_patterns::run(handler, args).await,
		_ => usage().to_string(),
	}
}

fn usage() -> &'static str {
	"booru subcommands: list, show <name>, add <name> <json>, edit <name> <field> <value>, delete <name>, enable <name>, disable <name>, parameter set|delete|list, tag-pattern add|remove|list"
}

async fn list(handler: &Handler) -> String {
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.get_all_boorus() {
		Ok(boorus) => {
			if boorus.is_empty() {
				"no boorus".to_string()
			} else {
				boorus
					.iter()
					.map(|b| {
						format!(
							"{} [{}]",
							b.name,
							if b.enabled { "enabled" } else { "disabled" }
						)
					})
					.collect::<Vec<_>>()
					.join("\n")
			}
		}
		Err(err) => format!("error: {err}"),
	}
}

async fn show(handler: &Handler, args: &str) -> String {
	let name = match super::single_arg(args, "usage: booru show <name>") {
		Ok(name) => name,
		Err(msg) => return msg,
	};
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.get_booru_by_name(name) {
		Ok(Some(booru)) => format(&booru),
		Ok(None) => format!("booru {name} not found"),
		Err(err) => format!("error: {err}"),
	}
}

async fn add(handler: &Handler, args: &str) -> String {
	let (name, json) = match super::split_required(args, "usage: booru add <name> <json>") {
		Ok(parts) => parts,
		Err(msg) => return msg,
	};
	let mut booru = match from_json(name, json) {
		Ok(booru) => booru,
		Err(err) => return format!("error: {err}"),
	};
	booru.name = name.to_string();
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.add_booru(&booru) {
		Ok(_) => format!("booru {name} added. restart bot to update commands."),
		Err(err) => format!("error: {err}"),
	}
}

async fn edit(handler: &Handler, args: &str) -> String {
	let (name, rest) = match super::split_required(args, "usage: booru edit <name> <field> <value>")
	{
		Ok(parts) => parts,
		Err(msg) => return msg,
	};
	let (field, value) =
		match super::split_required(rest, "usage: booru edit <name> <field> <value>") {
			Ok(parts) => parts,
			Err(msg) => return msg,
		};
	let db = handler.db.lock().expect("db mutex poisoned");
	let mut booru = match db.get_booru_by_name(name) {
		Ok(Some(booru)) => booru,
		Ok(None) => return format!("booru {name} not found"),
		Err(err) => return format!("error: {err}"),
	};
	if let Err(err) = edit_field(&mut booru, field, value) {
		return format!("error: {err}");
	}
	match db.update_booru(&booru) {
		Ok(()) => format!("booru {name} updated. restart bot to update commands."),
		Err(err) => format!("error: {err}"),
	}
}

async fn delete(handler: &Handler, args: &str) -> String {
	let name = match super::single_arg(args, "usage: booru delete <name>") {
		Ok(name) => name,
		Err(msg) => return msg,
	};
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.delete_booru(name) {
		Ok(()) => "booru deleted. restart bot to update commands.".to_string(),
		Err(err) => format!("error: {err}"),
	}
}

async fn enable(handler: &Handler, args: &str) -> String {
	let name = match super::single_arg(args, "usage: booru enable <name>") {
		Ok(name) => name,
		Err(msg) => return msg,
	};
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.set_booru_enabled(name, true) {
		Ok(()) => format!("booru {name} enabled. restart bot to update commands."),
		Err(err) => format!("error: {err}"),
	}
}

async fn disable(handler: &Handler, args: &str) -> String {
	let name = match super::single_arg(args, "usage: booru disable <name>") {
		Ok(name) => name,
		Err(msg) => return msg,
	};
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.set_booru_enabled(name, false) {
		Ok(()) => format!("booru {name} disabled. restart bot to update commands."),
		Err(err) => format!("error: {err}"),
	}
}

pub(crate) fn format(booru: &BooruRow) -> String {
	format!(
		"name: {}\nenabled: {}\nembed_image: {}\nmax_tags: {}\nsupports_character: {}\npage_size: {}\npage_base: {}\ntag_separator: {}\nencode_tag_separator: {}\ntag_spaces_as_plus: {}\ncharacter_space_replacement: {}\ncount_url: {}\ncount_path: {}\nposts_url: {}\nposts_path: {}\nfile_url_path: {}\nsource_url_path: {}\ndetail_url: {}\ndetail_id_path: {}\ndetail_file_url_path: {}\ndetail_source_url_path: {}\npost_url: {}\nheaders: {}\nenv_params: {}",
		booru.name,
		booru.enabled,
		booru.embed_image,
		booru.max_tags,
		booru.supports_character,
		booru.page_size,
		booru.page_base,
		booru.tag_separator,
		booru.encode_tag_separator,
		booru.tag_spaces_as_plus,
		booru.character_space_replacement,
		booru.count_url,
		booru.count_path_json,
		booru.posts_url,
		booru.posts_path_json,
		booru.file_url_path_json,
		booru.source_url_path_json,
		booru.detail_url.as_deref().unwrap_or("null"),
		booru.detail_id_path_json,
		booru.detail_file_url_path_json,
		booru.detail_source_url_path_json,
		booru.post_url.as_deref().unwrap_or("null"),
		booru.headers_json,
		booru.env_params_json,
	)
}

fn from_json(name: &str, json: &str) -> Result<BooruRow> {
	let mut value: serde_json::Value = serde_json::from_str(json).context("invalid booru json")?;
	let object = value
		.as_object_mut()
		.ok_or_else(|| anyhow::anyhow!("booru json must be an object"))?;
	let enabled = object
		.remove("enabled")
		.and_then(|value| value.as_bool())
		.unwrap_or(true);
	let supports_character = object
		.remove("supports_character")
		.and_then(|value| value.as_bool())
		.unwrap_or(false);
	object.insert(
		"name".to_string(),
		serde_json::Value::String(name.to_string()),
	);
	let config: BooruConfig = serde_json::from_value(value).context("invalid booru config")?;
	let row = BooruRow {
		id: 0,
		name: config.name,
		enabled,
		embed_image: config.embed_image,
		max_tags: config.max_tags,
		supports_character,
		page_size: config.page_size,
		page_base: config.page_base,
		tag_separator: config.tag_separator,
		encode_tag_separator: config.encode_tag_separator,
		tag_spaces_as_plus: config.tag_spaces_as_plus,
		character_space_replacement: config.character_space_replacement,
		count_url: config.count_url,
		count_path_json: serde_json::to_string(&config.count_path)?,
		posts_url: config.posts_url,
		posts_path_json: serde_json::to_string(&config.posts_path)?,
		file_url_path_json: serde_json::to_string(&config.file_url_path)?,
		source_url_path_json: serde_json::to_string(&config.source_url_path)?,
		detail_url: config.detail_url,
		detail_id_path_json: serde_json::to_string(&config.detail_id_path)?,
		detail_file_url_path_json: serde_json::to_string(&config.detail_file_url_path)?,
		detail_source_url_path_json: serde_json::to_string(&config.detail_source_url_path)?,
		post_url: config.post_url,
		headers_json: serde_json::to_string(&config.headers)?,
		env_params_json: serde_json::to_string(&config.env_params)?,
	};
	validate(&row)?;
	Ok(row)
}

fn edit_field(booru: &mut BooruRow, field: &str, value: &str) -> Result<()> {
	match field {
		"name" => booru.name = super::non_empty(value, field)?.to_string(),
		"enabled" => booru.enabled = super::parse_bool(value)?,
		"embed_image" => booru.embed_image = super::parse_bool(value)?,
		"max_tags" => booru.max_tags = value.parse().context("max_tags must be a number")?,
		"supports_character" => booru.supports_character = super::parse_bool(value)?,
		"page_size" => booru.page_size = value.parse().context("page_size must be a number")?,
		"page_base" => booru.page_base = value.parse().context("page_base must be a number")?,
		"tag_separator" => booru.tag_separator = value.to_string(),
		"encode_tag_separator" => booru.encode_tag_separator = super::parse_bool(value)?,
		"tag_spaces_as_plus" => booru.tag_spaces_as_plus = super::parse_bool(value)?,
		"character_space_replacement" => {
			booru.character_space_replacement = super::non_empty(value, field)?.to_string()
		}
		"count_url" => booru.count_url = super::non_empty(value, field)?.to_string(),
		"posts_url" => booru.posts_url = super::non_empty(value, field)?.to_string(),
		"detail_url" => booru.detail_url = super::optional_string(value),
		"post_url" => booru.post_url = super::optional_string(value),
		"count_path" => booru.count_path_json = super::normalized_json(value)?,
		"posts_path" => booru.posts_path_json = super::normalized_json(value)?,
		"file_url_path" => booru.file_url_path_json = super::normalized_json(value)?,
		"source_url_path" => booru.source_url_path_json = super::normalized_json(value)?,
		"detail_id_path" => booru.detail_id_path_json = super::normalized_json(value)?,
		"detail_file_url_path" => booru.detail_file_url_path_json = super::normalized_json(value)?,
		"detail_source_url_path" => {
			booru.detail_source_url_path_json = super::normalized_json(value)?
		}
		"headers" => booru.headers_json = super::normalized_json(value)?,
		"env_params" => booru.env_params_json = super::normalized_json(value)?,
		_ => bail!("unknown booru field {field}"),
	}
	validate(booru)
}

fn validate(booru: &BooruRow) -> Result<()> {
	if booru.name.trim().is_empty() {
		bail!("booru names cannot be empty");
	}
	if booru.page_size == 0 {
		bail!("page_size must be greater than zero");
	}
	if booru.character_space_replacement.is_empty() {
		bail!("character_space_replacement cannot be empty");
	}
	BooruConfig::from_row(booru).map(|_| ())
}
