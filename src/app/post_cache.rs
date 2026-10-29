use crate::{booru::ImageResult, db::Database};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Serialize, Deserialize)]
pub struct CachedPost {
	pub image: ImageResult,
	pub booru_name: String,
	pub embed_image: bool,
}

/// Persistent references, not image bytes. History pruning cascades to these rows.
pub struct PostCache {
	db: Arc<Mutex<Database>>,
}

impl PostCache {
	pub fn new(db: Arc<Mutex<Database>>) -> Self {
		Self { db }
	}
	pub fn store(&self, post: CachedPost, channel_id: i64) -> Result<String> {
		let id = unique_id();
		let db = self.db.lock().expect("db mutex poisoned");
		db.connection().execute(
			"INSERT INTO delivered_posts(id,channel_id,payload) VALUES(?1,?2,?3)",
			params![id, channel_id, serde_json::to_string(&post)?],
		)?;
		db.connection().execute("DELETE FROM delivered_posts WHERE id IN (SELECT id FROM delivered_posts WHERE channel_id=?1 AND art_id IS NULL ORDER BY created_at DESC, rowid DESC LIMIT -1 OFFSET ?2)", params![channel_id, db.history_limit() as i64])?;
		Ok(id)
	}
	pub fn get(&self, id: &str, channel_id: i64) -> Result<Option<CachedPost>> {
		let db = self.db.lock().expect("db mutex poisoned");
		let json: Option<String> = db.connection().query_row("SELECT payload FROM delivered_posts WHERE id=?1 AND channel_id=?2 AND (art_id IS NOT NULL OR created_at > unixepoch()-86400)", params![id,channel_id], |r| r.get(0)).optional()?;
		json.map(|value| serde_json::from_str(&value).map_err(Into::into))
			.transpose()
	}
	pub fn remove(&self, id: &str) -> Result<()> {
		self.db
			.lock()
			.expect("db mutex poisoned")
			.connection()
			.execute("DELETE FROM delivered_posts WHERE id=?1", [id])?;
		Ok(())
	}
	pub fn cleanup(&self) -> Result<()> {
		self.db
			.lock()
			.expect("db mutex poisoned")
			.connection()
			.execute(
				"DELETE FROM delivered_posts WHERE art_id IS NULL AND created_at <= unixepoch()-86400",
				[],
			)?;
		Ok(())
	}
}

pub(crate) fn unique_id() -> String {
	format!("{:032x}", rand::random::<u128>())
}
pub(crate) fn format_dm_content(post: &CachedPost) -> String {
	let mut lines = Vec::new();
	if let Some(url) = &post.image.post_url {
		lines.push(format!("[source ({})](<{url}>)", post.booru_name));
	}
	if let Some(url) = &post.image.upstream_source_url {
		lines.push(format!("[source](<{url}>)"));
	}
	if !post.embed_image {
		lines.push(post.image.image_url.clone());
	}
	lines.join("\n")
}
pub(crate) fn dm_button_id(id: &str) -> String {
	format!("dm:{id}")
}
pub(crate) fn parse_dm_button_id(id: &str) -> Option<&str> {
	id.strip_prefix("dm:")
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn ids_never_restart_at_zero() {
		let ids: std::collections::HashSet<_> = (0..1000).map(|_| unique_id()).collect();
		assert_eq!(ids.len(), 1000);
		assert!(ids.iter().all(|id| id.len() == 32));
	}
}
