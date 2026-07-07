use std::borrow::Cow;

use anyhow::{Context, Result, bail};
use image::{DynamicImage, Rgb, RgbImage, Rgba, codecs::jpeg::JpegEncoder};
use reqwest::Response;
use serde_json::Value;

use crate::config::{BooruConfig, JsonPathSegment, RuntimeValueSource, RuntimeValues};

pub(super) const INLINE_IMAGE_MAX_BYTES: usize = 10 * 1024 * 1024;

pub(super) fn compress_to_jpeg(bytes: &[u8]) -> Result<Vec<u8>> {
	let image = image::load_from_memory(bytes)
		.context("failed to decode oversized image for JPEG compression")?;
	let image = flatten_for_jpeg(image);
	let mut last = Vec::new();

	for quality in [90, 80, 70, 60, 50, 40, 30, 20] {
		let mut encoded = Vec::new();
		JpegEncoder::new_with_quality(&mut encoded, quality)
			.encode_image(&image)
			.context("failed to encode JPEG image")?;

		if encoded.len() <= INLINE_IMAGE_MAX_BYTES {
			return Ok(encoded);
		}

		last = encoded;
	}

	Ok(last)
}

pub(super) async fn ensure_success(
	response: Response,
	booru: &BooruConfig,
	kind: &str,
) -> Result<Response> {
	let status = response.status();
	if status.is_success() {
		return Ok(response);
	}

	let content_type = response_content_type(&response);
	let body = response.text().await.unwrap_or_default();
	let body = body.chars().take(500).collect::<String>();
	bail!(
		"{kind} request failed for {} with HTTP {status} content-type={content_type}: {body}",
		booru.name,
	);
}

pub(super) fn flatten_for_jpeg(image: DynamicImage) -> DynamicImage {
	let rgba = image.to_rgba8();
	let (width, height) = rgba.dimensions();
	let mut rgb = RgbImage::new(width, height);

	for (x, y, pixel) in rgba.enumerate_pixels() {
		rgb.put_pixel(x, y, Rgb(flatten_pixel(*pixel)));
	}

	DynamicImage::ImageRgb8(rgb)
}

pub(super) fn flatten_pixel(pixel: Rgba<u8>) -> [u8; 3] {
	let alpha = pixel[3] as u16;
	let blend =
		|channel: u8| -> u8 { (((channel as u16 * alpha) + (255 * (255 - alpha))) / 255) as u8 };

	[blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]
}

pub(super) fn infer_filename(url: &str, content_type: &str, compressed: bool) -> String {
	if compressed {
		return replace_extension(url_filename(url), "jpg");
	}

	let filename = url_filename(url);
	if filename.contains('.') {
		filename.into_owned()
	} else {
		format!("{}.{}", filename, extension_from_content_type(content_type))
	}
}

pub(super) fn url_filename(url: &str) -> Cow<'_, str> {
	let candidate = url
		.split('?')
		.next()
		.and_then(|value| value.rsplit('/').next())
		.filter(|value| !value.is_empty())
		.unwrap_or("image");

	Cow::Borrowed(candidate)
}

pub(super) fn replace_extension(filename: Cow<'_, str>, extension: &str) -> String {
	match filename.rsplit_once('.') {
		Some((stem, _)) if !stem.is_empty() => format!("{stem}.{extension}"),
		_ => format!("{}.{extension}", filename),
	}
}

pub(super) fn extension_from_content_type(content_type: &str) -> &'static str {
	match content_type.split(';').next().unwrap_or_default().trim() {
		"image/png" => "png",
		"image/gif" => "gif",
		"image/webp" => "webp",
		"image/bmp" => "bmp",
		_ => "jpg",
	}
}

pub(super) fn encode_tags(
	tags: &[String],
	separator: &str,
	encode_separator: bool,
	spaces_as_plus: bool,
) -> String {
	let separator = if encode_separator {
		encode_tag(separator, spaces_as_plus)
	} else {
		separator.to_string()
	};

	tags.iter()
		.map(|tag| encode_tag(tag, spaces_as_plus))
		.collect::<Vec<_>>()
		.join(&separator)
}

pub(super) fn response_content_type(response: &Response) -> String {
	response
		.headers()
		.get(reqwest::header::CONTENT_TYPE)
		.and_then(|value| value.to_str().ok())
		.unwrap_or("<missing>")
		.to_string()
}

pub(super) fn encode_tag(tag: &str, spaces_as_plus: bool) -> String {
	let encoded = urlencoding::encode(tag).into_owned();
	if spaces_as_plus {
		encoded.replace("%20", "+")
	} else {
		encoded
	}
}

pub(super) fn fill_template(
	booru: &BooruConfig,
	runtime_values: &RuntimeValues,
	template: &str,
	encoded_tags: &str,
	page: Option<u64>,
	limit: u64,
) -> String {
	let output = template
		.replace("{tags}", encoded_tags)
		.replace("{page}", &page.unwrap_or_default().to_string())
		.replace("{limit}", &limit.to_string());

	fill_runtime_template(booru, runtime_values, &output, true)
}

pub(super) fn fill_runtime_template(
	booru: &BooruConfig,
	runtime_values: &RuntimeValues,
	template: &str,
	encode_values: bool,
) -> String {
	let mut output = template.to_string();

	for env_param in &booru.env_params {
		output = output.replace(
			&format!("{{{}}}", env_param.placeholder),
			&runtime_placeholder(
				&booru.name,
				runtime_values,
				&env_param.source,
				&env_param.env,
				encode_values,
			),
		);
	}

	output
}

pub(super) fn runtime_placeholder(
	booru_name: &str,
	runtime_values: &RuntimeValues,
	source: &RuntimeValueSource,
	name: &str,
	encode: bool,
) -> String {
	let Some(value) = runtime_values.get(booru_name, source, name) else {
		if matches!(source, RuntimeValueSource::Custom) {
			crate::cli::warn_missing_custom(booru_name, name);
		}
		return String::new();
	};

	if encode {
		urlencoding::encode(value).into_owned()
	} else {
		value.to_string()
	}
}

pub(super) fn read_path<'a>(value: &'a Value, path: &[JsonPathSegment]) -> Option<&'a Value> {
	let mut current = value;

	for segment in path {
		current = match current {
			Value::Object(map) => match segment {
				JsonPathSegment::Key(key) => map.get(key)?,
				JsonPathSegment::Index(index) => map.get(&index.to_string())?,
			},
			Value::Array(array) => match segment {
				JsonPathSegment::Key(key) => array.get(key.parse::<usize>().ok()?)?,
				JsonPathSegment::Index(index) => array.get(*index)?,
			},
			_ => return None,
		};
	}

	Some(current)
}

pub(super) fn value_to_template_string(value: &Value) -> Option<String> {
	match value {
		Value::Number(number) => Some(number.to_string()),
		Value::String(string) => Some(string.to_string()),
		_ => None,
	}
}

pub(super) fn read_u64_path(value: &Value, path: &[JsonPathSegment]) -> Option<u64> {
	if path.is_empty() {
		return value.as_array().map(|array| array.len() as u64);
	}

	match read_path(value, path)? {
		Value::Number(number) => number.as_u64(),
		Value::String(string) => string.parse().ok(),
		Value::Array(array) => Some(array.len() as u64),
		_ => None,
	}
}

pub(super) fn read_array_path<'a>(
	value: &'a Value,
	path: &[JsonPathSegment],
) -> Option<&'a [Value]> {
	read_path(value, path)?
		.as_array()
		.map(|array| array.as_slice())
}

pub(super) fn read_string_path<'a>(value: &'a Value, path: &[JsonPathSegment]) -> Option<&'a str> {
	read_path(value, path)?.as_str()
}

pub(super) fn read_optional_string_path<'a>(
	value: &'a Value,
	path: &[JsonPathSegment],
) -> Option<&'a str> {
	if path.is_empty() {
		return None;
	}

	read_string_path(value, path).filter(|value| !value.trim().is_empty())
}

pub(super) fn post_url(
	booru: &BooruConfig,
	runtime_values: &RuntimeValues,
	post: &Value,
) -> Option<String> {
	let id = read_path(post, &booru.detail_id_path).and_then(value_to_template_string)?;

	post_url_from_id(booru, runtime_values, &id)
}

pub(super) fn post_url_from_id(
	booru: &BooruConfig,
	runtime_values: &RuntimeValues,
	id: &str,
) -> Option<String> {
	let template = booru.post_url.as_deref()?;

	Some(
		fill_template(booru, runtime_values, template, "", None, booru.page_size)
			.replace("{id}", &urlencoding::encode(id)),
	)
}

pub(super) fn detail_url_from_id(
	booru: &BooruConfig,
	runtime_values: &RuntimeValues,
	template: &str,
	id: &str,
) -> String {
	fill_template(booru, runtime_values, template, "", None, booru.page_size)
		.replace("{id}", &urlencoding::encode(id))
}

pub(super) fn reject_blacklisted_post(post: &Value, blacklisted_tags: &[String]) -> Result<()> {
	if blacklisted_tags.is_empty() {
		return Ok(());
	}

	let post_tags = extract_post_tags(post);
	if let Some(tag) = blacklisted_tags
		.iter()
		.find(|tag| post_tags.iter().any(|post_tag| post_tag == *tag))
	{
		bail!("post matched blacklisted tag: {tag}");
	}

	Ok(())
}

pub(super) fn extract_post_tags(post: &Value) -> Vec<String> {
	let mut tags = Vec::new();

	for key in [
		"tag_string",
		"tag_string_general",
		"tag_string_character",
		"tag_string_copyright",
		"tag_string_artist",
		"tag_string_meta",
		"tag_string_species",
		"tags",
		"tag_names",
	] {
		collect_tags_from_value(post.get(key), &mut tags);
	}

	tags.sort();
	tags.dedup();
	tags
}

pub(super) fn collect_tags_from_value(value: Option<&Value>, tags: &mut Vec<String>) {
	let Some(value) = value else {
		return;
	};

	match value {
		Value::String(value) => tags.extend(split_tag_string(value)),
		Value::Array(values) => {
			for value in values {
				collect_tags_from_value(Some(value), tags);
			}
		}
		Value::Object(map) => {
			for key in ["name", "tag", "value"] {
				collect_tags_from_value(map.get(key), tags);
			}
		}
		_ => {}
	}
}

pub(super) fn split_tag_string(value: &str) -> impl Iterator<Item = String> + '_ {
	value
		.split(|ch: char| ch.is_whitespace() || ch == ',')
		.map(str::trim)
		.filter(|tag| !tag.is_empty())
		.map(|tag| tag.to_ascii_lowercase())
}

pub(super) fn extract_upstream_source_url(value: &str) -> Option<String> {
	let trimmed = value.trim();

	reqwest::Url::parse(trimmed)
		.ok()
		.filter(|url| matches!(url.scheme(), "http" | "https"))
		.map(|url| url.to_string())
		.or_else(|| extract_first_http_url(trimmed))
}

pub(super) fn extract_first_http_url(value: &str) -> Option<String> {
	let start = value.find("http://").or_else(|| value.find("https://"))?;
	let candidate = value[start..]
		.split(|ch: char| ch.is_whitespace() || ['<', '>', '"', '\'', ')', ']'].contains(&ch))
		.next()?
		.trim_end_matches(|ch| ['.', ',', ';', ':'].contains(&ch));

	reqwest::Url::parse(candidate)
		.ok()
		.filter(|url| matches!(url.scheme(), "http" | "https"))
		.map(|url| url.to_string())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::HashMap;

	fn test_booru() -> BooruConfig {
		BooruConfig {
			name: "test".to_string(),
			embed_image: false,
			max_tags: 0,
			page_size: 100,
			page_base: 0,
			tag_separator: " ".to_string(),
			encode_tag_separator: true,
			tag_spaces_as_plus: false,
			character_space_replacement: "_".to_string(),
			count_url: Some("https://example.test/count?tags={tags}".to_string()),
			count_path: vec![JsonPathSegment::Key("count".to_string())],
			posts_url: "https://example.test/posts?tags={tags}&page={page}&limit={limit}"
				.to_string(),
			posts_path: vec![JsonPathSegment::Key("posts".to_string())],
			file_url_path: vec![JsonPathSegment::Key("file_url".to_string())],
			post_url: None,
			source_url_path: vec![],
			detail_url: None,
			detail_id_path: vec![],
			detail_file_url_path: vec![],
			detail_source_url_path: vec![],
			headers: std::collections::HashMap::new(),
			env_params: vec![],
		}
	}

	fn runtime_values() -> RuntimeValues {
		RuntimeValues {
			secrets: HashMap::from([
				("booru_login".to_string(), "user name".to_string()),
				("booru_api_key".to_string(), "key/value".to_string()),
			]),
			settings: HashMap::from([("user_agent_name".to_string(), "user name".to_string())]),
			custom: HashMap::from([(
				"test".to_string(),
				HashMap::from([("session".to_string(), "abc/123".to_string())]),
			)]),
		}
	}

	#[test]
	fn encodes_tags_individually_without_encoding_separator() {
		assert_eq!(
			encode_tags(
				&["Lumine".to_string(), "Blue Archive".to_string()],
				",",
				false,
				true
			),
			"Lumine,Blue+Archive"
		);
	}

	#[test]
	fn encodes_separator_for_query_parameters_by_default() {
		assert_eq!(
			encode_tags(
				&["rating:safe".to_string(), "blue archive".to_string()],
				" ",
				true,
				false,
			),
			"rating%3Asafe%20blue%20archive"
		);
	}

	#[test]
	fn extracts_direct_source_url() {
		assert_eq!(
			extract_upstream_source_url("https://source.example/path?source=test"),
			Some("https://source.example/path?source=test".to_string())
		);
	}

	#[test]
	fn extracts_embedded_source_url() {
		assert_eq!(
			extract_upstream_source_url("Source: <https://source.example/test/status/123>"),
			Some("https://source.example/test/status/123".to_string())
		);
	}

	#[test]
	fn extracts_blacklist_tags_from_common_post_shapes() {
		let post = serde_json::json!({
			"tag_string": "megane 1girl",
			"tag_string_artist": "foo_bar",
			"tags": ["solo", { "name": "artist_name" }]
		});

		let tags = extract_post_tags(&post);

		assert!(tags.contains(&"megane".to_string()));
		assert!(tags.contains(&"foo_bar".to_string()));
		assert!(tags.contains(&"artist_name".to_string()));
	}

	#[test]
	fn rejects_post_with_blacklisted_tag() {
		let post = serde_json::json!({
			"tag_string": "megane banned_tag"
		});

		let err = reject_blacklisted_post(&post, &["banned_tag".to_string()]).unwrap_err();

		assert!(err.to_string().contains("blacklisted tag: banned_tag"));
	}

	#[test]
	fn fills_url_env_placeholders_with_encoding() {
		let mut booru = test_booru();
		booru.env_params = vec![
			crate::config::EnvParamConfig {
				placeholder: "login".to_string(),
				env: "booru_login".to_string(),
				source: crate::config::RuntimeValueSource::Secrets,
			},
			crate::config::EnvParamConfig {
				placeholder: "api_key".to_string(),
				env: "booru_api_key".to_string(),
				source: crate::config::RuntimeValueSource::Secrets,
			},
		];

		let runtime_values = runtime_values();
		let url = fill_template(
			&booru,
			&runtime_values,
			"https://example.test?login={login}&api_key={api_key}",
			"rating%3Asafe",
			None,
			100,
		);

		assert_eq!(
			url,
			"https://example.test?login=user%20name&api_key=key%2Fvalue"
		);
	}

	#[test]
	fn fills_header_env_placeholders_without_url_encoding() {
		let mut booru = test_booru();
		booru.env_params = vec![crate::config::EnvParamConfig {
			placeholder: "name".to_string(),
			env: "user_agent_name".to_string(),
			source: crate::config::RuntimeValueSource::Settings,
		}];

		let runtime_values = runtime_values();

		assert_eq!(
			fill_runtime_template(
				&booru,
				&runtime_values,
				"Randobooru Discord Bot - {name}",
				false
			),
			"Randobooru Discord Bot - user name"
		);
	}

	#[test]
	fn fills_custom_booru_placeholders() {
		let mut booru = test_booru();
		booru.env_params = vec![crate::config::EnvParamConfig {
			placeholder: "session".to_string(),
			env: "session".to_string(),
			source: crate::config::RuntimeValueSource::Custom,
		}];

		let runtime_values = runtime_values();
		let url = fill_template(
			&booru,
			&runtime_values,
			"https://example.test?session={session}",
			"rating%3Asafe",
			None,
			100,
		);

		assert_eq!(url, "https://example.test?session=abc%2F123");
	}
}
