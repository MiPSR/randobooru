use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};

use rand::{RngExt, seq::SliceRandom};
use reqwest::{Client, Response, header::HeaderName};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli;
use crate::config::{BooruConfig, RuntimeValues};
use crate::pacing::ApiPacer;

mod helpers;

use helpers::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResult {
	pub image_url: String,
	pub post_url: Option<String>,
	pub upstream_source_url: Option<String>,
}

impl ImageResult {
	pub fn history_source(&self) -> Option<&str> {
		self.post_url
			.as_deref()
			.or(self.upstream_source_url.as_deref())
	}
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
	runtime_values: Arc<RuntimeValues>,
	pacers: Arc<Mutex<HashMap<String, (u64, ApiPacer)>>>,
	global_pacer: ApiPacer,
	image_workers: Arc<tokio::sync::Semaphore>,
}

impl BooruClient {
	pub fn new(pacer: ApiPacer, runtime_values: Arc<RuntimeValues>) -> Result<Self> {
		static IMAGE_WORKERS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
			std::sync::OnceLock::new();
		Ok(Self {
			http: Client::builder()
				.connect_timeout(Duration::from_secs(10))
				.timeout(Duration::from_secs(30))
				.redirect(reqwest::redirect::Policy::none())
				.build()?,
			runtime_values,
			pacers: Arc::new(Mutex::new(HashMap::new())),
			global_pacer: pacer,
			image_workers: Arc::clone(
				IMAGE_WORKERS.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2))),
			),
		})
	}

	pub fn with_runtime(&self, values: RuntimeValues) -> Self {
		let pace = values
			.settings
			.get("api_rate_pace_ms")
			.and_then(|v| v.parse().ok())
			.unwrap_or(0);
		Self {
			runtime_values: Arc::new(values),
			global_pacer: self.global_pacer.with_interval(Duration::from_millis(pace)),
			..self.clone()
		}
	}

	async fn pace(&self, booru: &BooruConfig) {
		let pacer = {
			let mut pacers = self.pacers.lock().expect("pacer mutex poisoned");
			let entry = pacers.entry(booru.name.clone()).or_insert_with(|| {
				(
					booru.pace_ms,
					ApiPacer::new(Duration::from_millis(booru.pace_ms)),
				)
			});
			if entry.0 != booru.pace_ms {
				*entry = (
					booru.pace_ms,
					ApiPacer::new(Duration::from_millis(booru.pace_ms)),
				);
			}
			entry.1.clone()
		};
		self.global_pacer.wait().await;
		pacer.wait().await;
	}

	pub async fn random_image(
		&self,
		booru: &BooruConfig,
		tags: &[String],
		blacklisted_tags: &[String],
		history_check: Option<&(dyn Fn(&str) -> bool + Send + Sync)>,
	) -> Result<ImageResult> {
		if tags.is_empty() {
			bail!("no tags provided for {}", booru.name);
		}

		for param in &booru.env_params {
			if matches!(param.source, crate::config::RuntimeValueSource::Custom) {
				let value = self
					.runtime_values
					.get(&booru.name, &param.source, &param.env);
				if value.map(|v| v.trim().is_empty()).unwrap_or(true) {
					bail!(
						"missing custom parameter `{}` for booru `{}`; set it via `booru parameter set {} {} <value>`",
						param.env,
						booru.name,
						booru.name,
						param.env
					);
				}
			}
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
			page_index(&mut rand::rng(), page_count) + booru.page_base
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
		let posts_json = self.request_json(booru, &posts_url, "posts").await?;

		let posts = read_array_path(&posts_json, &booru.posts_path)
			.with_context(|| format!("failed to read posts for {}", booru.name))?;
		if posts.is_empty() {
			bail!("{} returned an empty post page", booru.name);
		}
		let indices = post_order(&mut rand::rng(), posts.len());
		let mut last_error = anyhow::anyhow!("no eligible posts on this page");
		// Bound detail requests while trying alternatives on the fetched page.
		for index in indices.into_iter().take(10) {
			let post = &posts[index];
			if let Some(check) = history_check
				&& let Some(link) = post_url(booru, &self.runtime_values, post)
				&& check(&link)
			{
				continue;
			}
			match self.image_result(booru, post, blacklisted_tags).await {
				Ok(image) => {
					cli::booru_random("post", index);
					return Ok(image);
				}
				Err(err) => last_error = err,
			}
		}
		Err(last_error)
	}

	pub async fn inline_image(
		&self,
		booru: &BooruConfig,
		image_url: &str,
	) -> Result<InlineImageResult> {
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

		let bytes = bounded_body(response, 40 * 1024 * 1024).await?;

		if bytes.len() <= INLINE_IMAGE_MAX_BYTES {
			return Ok(InlineImageResult {
				filename: infer_filename(image_url, &content_type, false),
				data: bytes,
				compressed: false,
				compression_time: None,
			});
		}

		let compression_start = Instant::now();
		let permit = Arc::clone(&self.image_workers).acquire_owned().await?;
		let data = tokio::task::spawn_blocking(move || {
			let _permit = permit;
			compress_to_jpeg(&bytes)
		})
		.await??;
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
		let Some(detail_url) = &booru.detail_url else {
			reject_configured_blacklist(post, booru, blacklisted_tags)?;
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

		let detail_json = self.request_json(booru, &detail_url, "detail").await?;
		reject_configured_blacklist(&detail_json, booru, blacklisted_tags)?;
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
		let response = self.request_bytes(booru, url, kind).await?;

		let content_type = response_content_type(&response);
		let mime = content_type
			.split(';')
			.next()
			.unwrap_or_default()
			.trim()
			.to_ascii_lowercase();
		if mime != "application/json" && !mime.ends_with("+json") {
			bail!(
				"{kind} request for {} returned non-JSON content-type={content_type}",
				booru.name,
			);
		}

		let bytes = bounded_body(response, 8 * 1024 * 1024).await?;
		serde_json::from_slice(&bytes)
			.with_context(|| format!("invalid {kind} JSON from {}", booru.name))
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
		let mut url = reqwest::Url::parse(url).context("invalid request URL")?;
		anyhow::ensure!(
			matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
			"only HTTP(S) downloads are supported"
		);
		for _ in 0..=5 {
			anyhow::ensure!(
				url.username().is_empty() && url.password().is_none(),
				"request URLs must not contain embedded credentials"
			);
			self.pace(booru).await;
			cli::request(kind);
			let timeout = self
				.runtime_values
				.settings
				.get("http_timeout_secs")
				.and_then(|v| v.parse().ok())
				.unwrap_or(30);
			let mut request = self
				.http
				.get(url.clone())
				.timeout(Duration::from_secs(timeout));
			// API origins and explicitly trusted image/CDN origins can share authentication.
			if receives_headers(booru, &url) {
				request = self.apply_headers(booru, request)?;
			}
			if kind != "image" {
				request = request.header(reqwest::header::ACCEPT, "application/json");
			}
			let response = request.send().await.map_err(|err| {
				anyhow::anyhow!(
					"{kind} request for {} failed: {}",
					booru.name,
					err.without_url()
				)
			})?;
			if response.status().is_redirection() {
				let location = response
					.headers()
					.get(reqwest::header::LOCATION)
					.context("redirect without location")?
					.to_str()?;
				let next = url.join(location)?;
				anyhow::ensure!(
					matches!(next.scheme(), "http" | "https")
						&& !(url.scheme() == "https" && next.scheme() == "http"),
					"unsafe redirect"
				);
				url = next;
				continue;
			}
			return ensure_success(response, booru, kind).await;
		}
		bail!("too many redirects from {}", booru.name)
	}
}

fn receives_headers(booru: &BooruConfig, url: &reqwest::Url) -> bool {
	std::iter::once(&booru.posts_url)
		.chain(booru.count_url.iter())
		.chain(booru.detail_url.iter())
		.filter_map(|value| reqwest::Url::parse(value).ok())
		.any(|api| api.origin() == url.origin())
		|| booru
			.fetch_options
			.trusted_image_origins
			.iter()
			.any(|origin| *origin == url.origin().ascii_serialization())
}

fn page_index(rng: &mut impl rand::Rng, pages: u64) -> u64 {
	rng.random_range(0..pages)
}
fn post_order(rng: &mut impl rand::Rng, posts: usize) -> Vec<usize> {
	let mut indices = (0..posts).collect::<Vec<_>>();
	indices.shuffle(rng);
	indices
}

async fn bounded_body(mut response: Response, limit: usize) -> Result<Vec<u8>> {
	anyhow::ensure!(
		response
			.content_length()
			.is_none_or(|len| len <= limit as u64),
		"response exceeds download size limit"
	);
	let mut bytes = Vec::new();
	while let Some(chunk) = response
		.chunk()
		.await
		.map_err(|err| anyhow::anyhow!("response body read failed: {}", err.without_url()))?
	{
		anyhow::ensure!(
			bytes.len().saturating_add(chunk.len()) <= limit,
			"response exceeds download size limit"
		);
		bytes.extend_from_slice(&chunk);
	}
	Ok(bytes)
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncReadExt, AsyncWriteExt};
	#[test]
	fn page_and_post_draws_do_not_share_modulo_state() {
		use rand::SeedableRng;
		let mut rng = rand::rngs::StdRng::seed_from_u64(1234);
		let choices = (0..10_000)
			.map(|_| (page_index(&mut rng, 10), post_order(&mut rng, 10)[0]))
			.collect::<std::collections::HashSet<_>>();
		assert_eq!(choices.len(), 100); // The old same-token modulo algorithm reaches only ten.
	}

	async fn server(
		status: &str,
		headers: &str,
		body: &str,
	) -> (String, tokio::task::JoinHandle<String>) {
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let address = format!("http://{}", listener.local_addr().unwrap());
		let response = format!(
			"HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{headers}\r\n{body}",
			body.len()
		);
		let handle = tokio::spawn(async move {
			let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
				.await
				.unwrap()
				.unwrap();
			let mut bytes = Vec::new();
			let mut buffer = [0u8; 1024];
			while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
				let n = stream.read(&mut buffer).await.unwrap();
				if n == 0 {
					break;
				}
				bytes.extend_from_slice(&buffer[..n]);
				assert!(bytes.len() < 16384);
			}
			stream.write_all(response.as_bytes()).await.unwrap();
			String::from_utf8(bytes).unwrap()
		});
		(address, handle)
	}
	fn client() -> BooruClient {
		BooruClient::new(
			ApiPacer::new(Duration::ZERO),
			Arc::new(RuntimeValues {
				secrets: HashMap::new(),
				settings: HashMap::new(),
				custom: HashMap::new(),
			}),
		)
		.unwrap()
	}
	fn config(url: &str) -> BooruConfig {
		let mut b = crate::config::test_booru();
		b.posts_url = format!("{url}/posts?tags={{tags}}&page={{page}}");
		b.count_url = None;
		b.posts_path = vec![];
		b.post_url = Some(format!("{url}/post/{{id}}"));
		b.detail_id_path = vec![crate::config::JsonPathSegment::Key("id".into())];
		b
	}

	#[tokio::test]
	async fn fetch_tries_other_page_candidates_and_normalizes_blacklists() {
		let (url,request)=server("200 OK","",r#"[{"id":1,"file_url":"https://images.example/good.jpg","tags":{"general":["safe"]}},{"id":2,"file_url":"https://images.example/bad.jpg","tags":{"general":["blocked"]}}]"#).await;
		let image = client()
			.random_image(&config(&url), &["solo".into()], &["Blocked".into()], None)
			.await
			.unwrap();
		assert!(image.image_url.ends_with("good.jpg"));
		assert!(
			request
				.await
				.unwrap()
				.starts_with("GET /posts?tags=solo&page=1")
		);
	}
	#[tokio::test]
	async fn unknown_tag_shape_fails_closed() {
		let (url, request) = server(
			"200 OK",
			"",
			r#"[{"id":1,"file_url":"https://images.example/test.jpg"}]"#,
		)
		.await;
		let error = client()
			.random_image(&config(&url), &["solo".into()], &["blocked".into()], None)
			.await
			.unwrap_err();
		assert!(error.to_string().contains("cannot inspect"));
		request.await.unwrap();
	}
	#[tokio::test]
	async fn redirects_do_not_leak_headers_to_other_origins() {
		let (image_url, image_request) = server("200 OK", "", "{}").await;
		let (api_url, api_request) =
			server("302 Found", &format!("Location: {image_url}/image\r\n"), "").await;
		let mut b = config(&api_url);
		b.headers
			.insert("Authorization".into(), "Bearer audit-secret".into());
		client()
			.request_bytes(&b, &format!("{api_url}/image"), "image")
			.await
			.unwrap();
		assert!(
			api_request
				.await
				.unwrap()
				.to_ascii_lowercase()
				.contains("authorization: bearer audit-secret")
		);
		assert!(
			!image_request
				.await
				.unwrap()
				.to_ascii_lowercase()
				.contains("audit-secret")
		);
	}
	#[tokio::test]
	async fn trusted_image_origin_receives_required_headers() {
		let (url, request) = server("200 OK", "", "{}").await;
		let mut b = config("https://api.example");
		b.fetch_options.trusted_image_origins.push(url.clone());
		b.headers
			.insert("Authorization".into(), "Bearer audit-secret".into());
		client().request_bytes(&b, &url, "image").await.unwrap();
		assert!(
			request
				.await
				.unwrap()
				.to_ascii_lowercase()
				.contains("authorization: bearer audit-secret")
		);
	}
	#[tokio::test]
	async fn body_limits_reject_oversized_responses() {
		let (url, request) = server("200 OK", "", "12345").await;
		let response = client()
			.request_bytes(&config(&url), &url, "posts")
			.await
			.unwrap();
		assert!(bounded_body(response, 4).await.is_err());
		request.await.unwrap();
	}

	#[tokio::test]
	async fn remote_error_bodies_and_query_credentials_are_not_reported() {
		let (url, request) = server("401 Unauthorized", "", "audit-canary-secret").await;
		let error = client()
			.request_bytes(
				&config(&url),
				&format!("{url}/?token=audit-query-secret"),
				"posts",
			)
			.await
			.unwrap_err();
		let text = format!("{error:#}");
		assert!(!text.contains("audit-canary-secret"));
		assert!(!text.contains("audit-query-secret"));
		request.await.unwrap();
	}
	#[tokio::test]
	async fn concurrent_requests_share_provider_pacing() {
		let (a, ra) = server("200 OK", "", "{}").await;
		let (b, rb) = server("200 OK", "", "{}").await;
		let mut config = config(&a);
		config.pace_ms = 40;
		let client = client();
		let start = Instant::now();
		let (one, two) = tokio::join!(
			client.request_bytes(&config, &a, "posts"),
			client.request_bytes(&config, &b, "posts")
		);
		one.unwrap();
		two.unwrap();
		assert!(start.elapsed() >= Duration::from_millis(35));
		ra.await.unwrap();
		rb.await.unwrap();
	}
}
