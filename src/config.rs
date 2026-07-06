use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::db::BooruRow;

#[derive(Debug, Clone, Deserialize)]
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
	pub count_url: String,
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
		Ok(Self {
			name: row.name.clone(),
			embed_image: row.embed_image,
			max_tags: row.max_tags,
			page_size: row.page_size,
			page_base: row.page_base,
			tag_separator: row.tag_separator.clone(),
			encode_tag_separator: row.encode_tag_separator,
			tag_spaces_as_plus: row.tag_spaces_as_plus,
			character_space_replacement: row.character_space_replacement.clone(),
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
		})
	}

	#[cfg(test)]
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
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn test_booru(name: &str) -> BooruConfig {
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
			count_url: "https://example.test/count?tags={tags}".to_string(),
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
