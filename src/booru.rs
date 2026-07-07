use std::{sync::Arc, time::Instant};

use anyhow::{Context, Result, bail};
use rand::RngExt;
use reqwest::{Client, Response, header::HeaderName};
use serde_json::Value;

use crate::cli;
use crate::config::{BooruConfig, RuntimeValues};
use crate::pacing::ApiPacer;

mod helpers;

use helpers::*;

#[derive(Clone)]
pub struct ImageResult {
	pub image_url: String,
	pub post_url: Option<String>,
	pub upstream_source_url: Option<String>,
}

pub struct InlineImageResult {
	pub filename: String,
	pub data: Vec<u8>,
	pub compressed: bool,
	pub compression_time: Option<String>,
}

#[derive(Clone)]
pub struct BooruClient {
	http: Client,
	pacer: ApiPacer,
	runtime_values: Arc<RuntimeValues>,
}

impl BooruClient {
	pub fn new(pacer: ApiPacer, runtime_values: Arc<RuntimeValues>) -> Self {
		Self {
			http: Client::new(),
			pacer,
			runtime_values,
		}
	}

	pub async fn random_image(
		&self,
		booru: &BooruConfig,
		tags: &[String],
		blacklisted_tags: &[String],
	) -> Result<ImageResult> {
		if tags.is_empty() {
			bail!("no tags provided for {}", booru.name);
		}

		let encoded_tags = encode_tags(
			tags,
			&booru.tag_separator,
			booru.encode_tag_separator,
			booru.tag_spaces_as_plus,
		);

		let count = if let Some(ref count_url) = booru.count_url {
			let count_url = fill_template(
				booru,
				&self.runtime_values,
				count_url,
				&encoded_tags,
				None,
				booru.page_size,
			);
			self.pacer.wait().await;
			let count_json = self.request_json(booru, &count_url, "count").await?;

			read_u64_path(&count_json, &booru.count_path)
				.with_context(|| format!("failed to read count for {}", booru.name))?
		} else {
			0
		};

		let random_page = if count == 0 && booru.count_url.is_none() {
			booru.page_base
		} else {
			if count == 0 {
				bail!("{} has no posts for tags: {}", booru.name, tags.join(" "));
			}
			let page_count = count.div_ceil(booru.page_size);
			rand::rng().random_range(0..page_count) + booru.page_base
		};
		cli::booru_random("page", random_page);
		let posts_url = fill_template(
			booru,
			&self.runtime_values,
			&booru.posts_url,
			&encoded_tags,
			Some(random_page),
			booru.page_size,
		);
		self.pacer.wait().await;
		let posts_json = self.request_json(booru, &posts_url, "posts").await?;

		let posts = read_array_path(&posts_json, &booru.posts_path)
			.with_context(|| format!("failed to read posts for {}", booru.name))?;
		if posts.is_empty() {
			bail!("{} returned an empty post page", booru.name);
		}
		let post_index = rand::rng().random_range(0..posts.len());
		cli::booru_random("post", post_index);
		let post = &posts[post_index];
		self.image_result(booru, post, blacklisted_tags).await
	}

	pub async fn inline_image(
		&self,
		booru: &BooruConfig,
		image_url: &str,
	) -> Result<InlineImageResult> {
		self.pacer.wait().await;
		let response = self.request_bytes(booru, image_url, "image").await?;
		let content_type = response_content_type(&response).to_string();

		if content_type != "<missing>"
			&& !content_type.starts_with("image/")
			&& !content_type.starts_with("application/octet-stream")
		{
			bail!(
				"image request for {} returned non-image content-type={content_type}",
				booru.name
			);
		}

		let bytes = response
			.bytes()
			.await
			.with_context(|| format!("failed to read image body from {}", booru.name))?
			.to_vec();

		if bytes.len() <= INLINE_IMAGE_MAX_BYTES {
			return Ok(InlineImageResult {
				filename: infer_filename(image_url, &content_type, false),
				data: bytes,
				compressed: false,
				compression_time: None,
			});
		}

		let compression_start = Instant::now();
		let data = compress_to_jpeg(&bytes)?;
		let elapsed = compression_start.elapsed();
		Ok(InlineImageResult {
			filename: infer_filename(image_url, &content_type, true),
			data,
			compressed: true,
			compression_time: Some(format!("{}ms", elapsed.as_millis())),
		})
	}

	async fn image_result(
		&self,
		booru: &BooruConfig,
		post: &Value,
		blacklisted_tags: &[String],
	) -> Result<ImageResult> {
		reject_blacklisted_post(post, blacklisted_tags)?;

		let Some(detail_url) = &booru.detail_url else {
			let image_url = read_string_path(post, &booru.file_url_path)
				.with_context(|| format!("failed to read file URL for {}", booru.name))?;
			let upstream_source_url = read_optional_string_path(post, &booru.source_url_path)
				.and_then(extract_upstream_source_url);
			return Ok(ImageResult {
				image_url: image_url.to_string(),
				post_url: post_url(booru, &self.runtime_values, post),
				upstream_source_url,
			});
		};

		let id = read_path(post, &booru.detail_id_path)
			.and_then(value_to_template_string)
			.with_context(|| format!("failed to read detail ID for {}", booru.name))?;
		let detail_url = detail_url_from_id(booru, &self.runtime_values, detail_url, &id);

		self.pacer.wait().await;
		let detail_json = self.request_json(booru, &detail_url, "detail").await?;
		reject_blacklisted_post(&detail_json, blacklisted_tags)?;
		let image_url = read_string_path(&detail_json, &booru.detail_file_url_path)
			.with_context(|| format!("failed to read detail file URL for {}", booru.name))?;
		let upstream_source_url =
			read_optional_string_path(&detail_json, &booru.detail_source_url_path)
				.or_else(|| read_optional_string_path(post, &booru.source_url_path))
				.and_then(extract_upstream_source_url);

		Ok(ImageResult {
			image_url: image_url.to_string(),
			post_url: post_url_from_id(booru, &self.runtime_values, &id)
				.or_else(|| post_url(booru, &self.runtime_values, post)),
			upstream_source_url,
		})
	}

	async fn request_json(&self, booru: &BooruConfig, url: &str, kind: &str) -> Result<Value> {
		cli::request(url);
		let request = self
			.apply_headers(booru, self.http.get(url))?
			.header(reqwest::header::ACCEPT, "application/json");

		let response = request
			.send()
			.await
			.with_context(|| format!("failed to request {kind} from {}", booru.name))?;
		let response = ensure_success(response, booru, kind).await?;

		let content_type = response_content_type(&response);
		if !content_type.contains("application/json") {
			let body = response.text().await.unwrap_or_default();
			let body = body.chars().take(500).collect::<String>();
			bail!(
				"{kind} request for {} returned non-JSON content-type={content_type}: {body}",
				booru.name,
			);
		}

		response
			.json()
			.await
			.with_context(|| format!("failed to decode {kind} response from {}", booru.name))
	}

	fn apply_headers(
		&self,
		booru: &BooruConfig,
		mut request: reqwest::RequestBuilder,
	) -> Result<reqwest::RequestBuilder> {
		for (name, value) in &booru.headers {
			let name = HeaderName::from_bytes(name.as_bytes())
				.with_context(|| format!("invalid header name {name} for {}", booru.name))?;
			request = request.header(
				name,
				fill_runtime_template(booru, &self.runtime_values, value, false),
			);
		}

		Ok(request)
	}

	async fn request_bytes(&self, booru: &BooruConfig, url: &str, kind: &str) -> Result<Response> {
		cli::request(url);
		let request = self.apply_headers(booru, self.http.get(url))?;

		let response = request
			.send()
			.await
			.with_context(|| format!("failed to request {kind} from {}", booru.name))?;

		ensure_success(response, booru, kind).await
	}
}
