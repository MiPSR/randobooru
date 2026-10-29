use std::collections::HashMap;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::db::BooruRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BooruConfig {
	pub name: String,
	pub embed_image: bool,
	pub max_tags: usize,
	pub page_size: u64,
	pub page_base: u64,
	pub tag_separator: String,
	pub encode_tag_separator: bool,
	pub tag_spaces_as_plus: bool,
	pub character_space_replacement: String,
	#[serde(default)]
	pub pace_ms: u64,
	pub count_url: Option<String>,
	pub count_path: Vec<JsonPathSegment>,
	pub posts_url: String,
	pub posts_path: Vec<JsonPathSegment>,
	pub file_url_path: Vec<JsonPathSegment>,
	pub post_url: Option<String>,
	pub source_url_path: Vec<JsonPathSegment>,
	pub detail_url: Option<String>,
	pub detail_id_path: Vec<JsonPathSegment>,
	pub detail_file_url_path: Vec<JsonPathSegment>,
	pub detail_source_url_path: Vec<JsonPathSegment>,
	pub headers: HashMap<String, String>,
	pub env_params: Vec<EnvParamConfig>,
	#[serde(default)]
	pub fetch_options: FetchOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FetchOptions {
	/// Additional exact origins allowed to receive the configured API headers.
	pub trusted_image_origins: Vec<String>,
	/// Paths relative to a post (or its detail document) containing tag data.
	pub tag_paths: Vec<Vec<JsonPathSegment>>,
}

impl Default for FetchOptions {
	fn default() -> Self {
		Self {
			trusted_image_origins: Vec::new(),
			tag_paths: [
				"tags",
				"tag_string",
				"tag_string_general",
				"tag_string_character",
				"tag_string_copyright",
				"tag_string_artist",
				"tag_string_meta",
				"tag_string_species",
				"tag_names",
			]
			.into_iter()
			.map(|key| vec![JsonPathSegment::Key(key.into())])
			.collect(),
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonPathSegment {
	Key(String),
	Index(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeValueSource {
	Secrets,
	Settings,
	Custom,
}

#[derive(Debug, Clone)]
pub struct RuntimeValues {
	pub secrets: HashMap<String, String>,
	pub settings: HashMap<String, String>,
	pub custom: HashMap<String, HashMap<String, String>>,
}

impl RuntimeValues {
	pub fn get(&self, booru_name: &str, source: &RuntimeValueSource, key: &str) -> Option<&str> {
		match source {
			RuntimeValueSource::Secrets => self.secrets.get(key).map(String::as_str),
			RuntimeValueSource::Settings => self.settings.get(key).map(String::as_str),
			RuntimeValueSource::Custom => self
				.custom
				.get(booru_name)
				.and_then(|params| params.get(key))
				.map(String::as_str),
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvParamConfig {
	pub placeholder: String,
	pub env: String,
	pub source: RuntimeValueSource,
}

impl BooruConfig {
	pub fn from_row(row: &BooruRow) -> Result<Self> {
		let config = Self {
			name: row.name.clone(),
			embed_image: row.embed_image,
			max_tags: row.max_tags,
			page_size: row.page_size,
			page_base: row.page_base,
			tag_separator: row.tag_separator.clone(),
			encode_tag_separator: row.encode_tag_separator,
			tag_spaces_as_plus: row.tag_spaces_as_plus,
			character_space_replacement: row.character_space_replacement.clone(),
			pace_ms: row.pace_ms,
			count_url: row.count_url.clone(),
			count_path: serde_json::from_str(&row.count_path_json)
				.context("failed to parse count_path_json")?,
			posts_url: row.posts_url.clone(),
			posts_path: serde_json::from_str(&row.posts_path_json)
				.context("failed to parse posts_path_json")?,
			file_url_path: serde_json::from_str(&row.file_url_path_json)
				.context("failed to parse file_url_path_json")?,
			post_url: row.post_url.clone(),
			source_url_path: serde_json::from_str(&row.source_url_path_json)
				.context("failed to parse source_url_path_json")?,
			detail_url: row.detail_url.clone(),
			detail_id_path: serde_json::from_str(&row.detail_id_path_json)
				.context("failed to parse detail_id_path_json")?,
			detail_file_url_path: serde_json::from_str(&row.detail_file_url_path_json)
				.context("failed to parse detail_file_url_path_json")?,
			detail_source_url_path: serde_json::from_str(&row.detail_source_url_path_json)
				.context("failed to parse detail_source_url_path_json")?,
			headers: serde_json::from_str(&row.headers_json)
				.context("failed to parse headers_json")?,
			env_params: serde_json::from_str(&row.env_params_json)
				.context("failed to parse env_params_json")?,
			fetch_options: serde_json::from_str(&row.fetch_options_json)
				.context("invalid fetch options")?,
		};
		config.validate()?;
		Ok(config)
	}

	pub fn validate(&self) -> Result<()> {
		ensure!(
			!crate::app::discord_name_component(&self.name).is_empty(),
			"provider name must contain letters or digits"
		);
		validate_command_name(&crate::app::custom_command_name(&self.name))?;
		ensure!(
			(1..=1000).contains(&self.page_size),
			"page_size must be 1..1000"
		);
		ensure!(self.max_tags <= 25, "max_tags must be 0..25");
		ensure!(self.page_base <= 1_000_000, "page_base is too large");
		ensure!(self.pace_ms <= 60_000, "pace_ms must be at most 60000");
		ensure!(
			!self.character_space_replacement.is_empty(),
			"character_space_replacement cannot be empty"
		);
		for template in std::iter::once(&self.posts_url)
			.chain(self.count_url.iter())
			.chain(self.detail_url.iter())
			.chain(self.post_url.iter())
		{
			let url = reqwest::Url::parse(template).context("invalid URL template")?;
			ensure!(
				matches!(url.scheme(), "https" | "http") && url.host_str().is_some(),
				"URLs must use HTTP(S)"
			);
			ensure!(
				url.username().is_empty() && url.password().is_none(),
				"put URL credentials in custom parameters instead"
			);
		}
		for origin in &self.fetch_options.trusted_image_origins {
			let url = reqwest::Url::parse(origin).context("invalid trusted image origin")?;
			ensure!(
				matches!(url.scheme(), "https" | "http")
					&& url.origin().ascii_serialization() == *origin,
				"trusted image entries must be exact origins, e.g. https://images.example.org"
			);
		}
		let mut names = std::collections::HashSet::new();
		for param in &self.env_params {
			ensure!(
				!param.placeholder.is_empty()
					&& param
						.placeholder
						.chars()
						.all(|c| c.is_ascii_alphanumeric() || c == '_'),
				"invalid placeholder"
			);
			ensure!(
				!["tags", "page", "limit", "id"].contains(&param.placeholder.as_str())
					&& names.insert(&param.placeholder),
				"duplicate or reserved placeholder"
			);
		}
		for (name, value) in &self.headers {
			reqwest::header::HeaderName::from_bytes(name.as_bytes())
				.context("invalid configured header name")?;
			ensure!(
				!value.contains(['\r', '\n']),
				"header values cannot contain newlines"
			);
		}
		Ok(())
	}

	pub fn to_row(&self) -> Result<BooruRow> {
		Ok(BooruRow {
			id: 0,
			name: self.name.clone(),
			enabled: true,
			embed_image: self.embed_image,
			max_tags: self.max_tags,
			supports_character: false,
			page_size: self.page_size,
			page_base: self.page_base,
			tag_separator: self.tag_separator.clone(),
			encode_tag_separator: self.encode_tag_separator,
			tag_spaces_as_plus: self.tag_spaces_as_plus,
			character_space_replacement: self.character_space_replacement.clone(),
			pace_ms: self.pace_ms,
			description: String::new(),
			count_url: self.count_url.clone(),
			count_path_json: serde_json::to_string(&self.count_path)
				.context("failed to serialize count_path")?,
			posts_url: self.posts_url.clone(),
			posts_path_json: serde_json::to_string(&self.posts_path)
				.context("failed to serialize posts_path")?,
			file_url_path_json: serde_json::to_string(&self.file_url_path)
				.context("failed to serialize file_url_path")?,
			source_url_path_json: serde_json::to_string(&self.source_url_path)
				.context("failed to serialize source_url_path")?,
			detail_url: self.detail_url.clone(),
			detail_id_path_json: serde_json::to_string(&self.detail_id_path)
				.context("failed to serialize detail_id_path")?,
			detail_file_url_path_json: serde_json::to_string(&self.detail_file_url_path)
				.context("failed to serialize detail_file_url_path")?,
			detail_source_url_path_json: serde_json::to_string(&self.detail_source_url_path)
				.context("failed to serialize detail_source_url_path")?,
			post_url: self.post_url.clone(),
			headers_json: serde_json::to_string(&self.headers)
				.context("failed to serialize headers")?,
			env_params_json: serde_json::to_string(&self.env_params)
				.context("failed to serialize env_params")?,
			fetch_options_json: serde_json::to_string(&self.fetch_options)?,
		})
	}
}

pub fn validate_command_name(name: &str) -> Result<()> {
	ensure!(
		(1..=32).contains(&name.len())
			&& name
				.bytes()
				.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
		"slash command names must normalize to 1..32 ASCII letters, digits or hyphens"
	);
	Ok(())
}

#[cfg(test)]
pub(crate) fn test_booru() -> BooruConfig {
	tests::test_booru("test")
}

#[cfg(test)]
mod tests {
	use super::*;

	pub(super) fn test_booru(name: &str) -> BooruConfig {
		BooruConfig {
			name: name.to_string(),
			embed_image: false,
			max_tags: 0,
			page_size: 100,
			page_base: 1,
			tag_separator: ",".to_string(),
			encode_tag_separator: true,
			tag_spaces_as_plus: false,
			character_space_replacement: "_".to_string(),
			pace_ms: 0,
			count_url: Some("https://example.test/count?tags={tags}".to_string()),
			count_path: vec![],
			posts_url: "https://example.test/posts?tags={tags}&page={page}&limit={limit}"
				.to_string(),
			posts_path: vec![],
			file_url_path: vec![JsonPathSegment::Key("file_url".to_string())],
			post_url: None,
			source_url_path: vec![],
			detail_url: None,
			detail_id_path: vec![],
			detail_file_url_path: vec![],
			detail_source_url_path: vec![],
			headers: HashMap::new(),
			env_params: vec![],
			fetch_options: FetchOptions::default(),
		}
	}

	#[test]
	fn booru_config_roundtrip_via_row() {
		let booru = test_booru("roundtrip");
		let row = booru.to_row().expect("to_row");
		let back = BooruConfig::from_row(&row).expect("from_row");

		assert_eq!(booru.name, back.name);
		assert_eq!(booru.page_size, back.page_size);
		assert_eq!(booru.tag_separator, back.tag_separator);
		assert_eq!(booru.count_url, back.count_url);
	}
}
