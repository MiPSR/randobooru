use std::{
	collections::HashMap,
	sync::{
		Mutex,
		atomic::{AtomicU64, Ordering},
	},
	time::{Duration, Instant},
};

use crate::booru::ImageResult;

pub struct CachedPost {
	pub image: ImageResult,
	pub booru_name: String,
	pub embed_image: bool,
	pub inline_data: Option<Vec<u8>>,
	pub inline_filename: Option<String>,
}

pub struct PostCache {
	posts: Mutex<HashMap<String, (CachedPost, Instant)>>,
	counter: AtomicU64,
	max_age: Duration,
}

impl PostCache {
	pub fn new() -> Self {
		Self {
			posts: Mutex::new(HashMap::new()),
			counter: AtomicU64::new(0),
			max_age: Duration::from_secs(3600),
		}
	}

	pub fn store(&self, post: CachedPost) -> String {
		let id = self.counter.fetch_add(1, Ordering::AcqRel).to_string();
		let mut posts = self.posts.lock().expect("post cache mutex poisoned");
		posts.insert(id.clone(), (post, Instant::now()));
		id
	}

	pub fn get(&self, id: &str) -> Option<CachedPost> {
		let posts = self.posts.lock().expect("post cache mutex poisoned");
		if let Some((post, _)) = posts.get(id) {
			return Some(CachedPost {
				image: ImageResult {
					image_url: post.image.image_url.clone(),
					post_url: post.image.post_url.clone(),
					upstream_source_url: post.image.upstream_source_url.clone(),
				},
				booru_name: post.booru_name.clone(),
				embed_image: post.embed_image,
				inline_data: post.inline_data.clone(),
				inline_filename: post.inline_filename.clone(),
			});
		}
		None
	}

	pub fn cleanup(&self) {
		let now = Instant::now();
		let mut posts = self.posts.lock().expect("post cache mutex poisoned");
		posts.retain(|_, (_, timestamp)| now.duration_since(*timestamp) < self.max_age);
	}
}

impl Default for PostCache {
	fn default() -> Self {
		Self::new()
	}
}

pub(crate) fn format_dm_content(post: &CachedPost) -> String {
	let mut lines = Vec::new();
	if let Some(post_url) = &post.image.post_url {
		lines.push(format!("[source ({})](<{}>)", post.booru_name, post_url));
	}
	if let Some(source) = &post.image.upstream_source_url {
		lines.push(format!("[source](<{source}>)"));
	}
	if !lines.is_empty() {
		lines.push(String::new());
	}
	lines.push(post.image.image_url.clone());
	lines.join("\n")
}

pub(crate) fn dm_button_id(cache_id: &str) -> String {
	format!("dm:{cache_id}")
}

pub(crate) fn parse_dm_button_id(custom_id: &str) -> Option<&str> {
	custom_id.strip_prefix("dm:")
}
