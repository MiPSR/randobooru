use anyhow::Result;
use serenity::all::{
	ButtonStyle, CommandInteraction, CreateActionRow, CreateAttachment, CreateButton,
	EditInteractionResponse, ReactionType,
};

use crate::{
	app::post_cache::{CachedPost, PostCache, dm_button_id},
	booru::{ImageResult, InlineImageResult},
	cli,
	config::BooruConfig,
	pacing::ApiPacer,
};

pub(crate) fn discord_name_component(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	let mut last_was_hyphen = false;

	for ch in value.chars().flat_map(|ch| ch.to_lowercase()) {
		let mapped = if ch.is_ascii_alphanumeric() { ch } else { '-' };
		if mapped == '-' {
			if last_was_hyphen {
				continue;
			}
			last_was_hyphen = true;
		} else {
			last_was_hyphen = false;
		}
		out.push(mapped);
	}

	out.trim_matches('-').to_string()
}

pub(crate) fn custom_command_name(booru_name: &str) -> String {
	format!("{}-custom", discord_name_component(booru_name))
}

pub(crate) fn custom_tag_count(booru: &BooruConfig) -> usize {
	if booru.max_tags == 0 {
		9
	} else {
		booru.max_tags
	}
}

pub(crate) fn normalize_user_tag(tag: &str, space_replacement: &str) -> String {
	tag.trim().replace(' ', space_replacement)
}

pub(crate) fn format_image_response(
	booru: &BooruConfig,
	image: &ImageResult,
	compressed: bool,
) -> String {
	let links = response_source_links(booru, image, compressed);

	if links.is_empty() {
		image.image_url.clone()
	} else {
		format!("{}\n{}", links.join(" | "), image.image_url)
	}
}

pub(crate) fn format_inline_image_response(
	booru: &BooruConfig,
	image: &ImageResult,
	compressed: bool,
) -> Option<String> {
	let links = response_source_links(booru, image, compressed);
	(!links.is_empty()).then(|| links.join(" | "))
}

fn response_source_links(
	booru: &BooruConfig,
	image: &ImageResult,
	compressed: bool,
) -> Vec<String> {
	let mut links = Vec::new();

	if let Some(post_url) = &image.post_url {
		links.push(format!("[source ({})](<{post_url}>)", booru.name));
	}

	if let Some(upstream_source_url) = &image.upstream_source_url {
		links.push(format_source_link(upstream_source_url));
	}

	if compressed {
		links.push("compressed".to_string());
	}

	links
}

fn format_source_link(url: &str) -> String {
	let label = source_site_label(url)
		.map(|site| format!("source ({site})"))
		.unwrap_or_else(|| "source".to_string());

	format!("[{label}](<{url}>)")
}

fn source_site_label(url: &str) -> Option<String> {
	let hostname = reqwest::Url::parse(url)
		.ok()?
		.host_str()?
		.trim_start_matches("www.")
		.to_ascii_lowercase();

	Some(hostname)
}

pub(crate) fn format_recent_art_links(links: &[String]) -> String {
	links
		.iter()
		.enumerate()
		.map(|(index, link)| format!("{}. <{link}>", index + 1))
		.collect::<Vec<_>>()
		.join("\n")
}

pub(crate) async fn edit_interaction(
	http: &serenity::http::Http,
	pacer: &ApiPacer,
	command: &CommandInteraction,
	content: impl Into<String>,
) -> Result<serenity::model::channel::Message> {
	pacer.wait().await;
	let message = command
		.edit_response(http, EditInteractionResponse::new().content(content))
		.await
		.map_err(anyhow::Error::from)?;
	cli::app_output(
		command.guild_id.map(|g| g.get() as i64),
		command.channel_id.get() as i64,
		"sent_message",
	);
	Ok(message)
}

pub(crate) async fn edit_interaction_with_dm(
	http: &serenity::http::Http,
	pacer: &ApiPacer,
	cache: &PostCache,
	command: &CommandInteraction,
	booru: &BooruConfig,
	image: &ImageResult,
) -> Result<serenity::model::channel::Message> {
	pacer.wait().await;

	let cache_id = cache.store(CachedPost {
		image: image.clone(),
		booru_name: booru.name.clone(),
		embed_image: booru.embed_image,
		inline_data: None,
		inline_filename: None,
	});

	let content = format_image_response(booru, image, false);
	let button = CreateButton::new(dm_button_id(&cache_id))
		.label("Send to DM")
		.emoji(ReactionType::Unicode("📬".to_string()))
		.style(ButtonStyle::Secondary);

	let message = command
		.edit_response(
			http,
			EditInteractionResponse::new()
				.content(content)
				.components(vec![CreateActionRow::Buttons(vec![button])]),
		)
		.await
		.map_err(anyhow::Error::from)?;
	cli::app_output(
		command.guild_id.map(|g| g.get() as i64),
		command.channel_id.get() as i64,
		"sent_message",
	);
	Ok(message)
}

pub(crate) async fn edit_interaction_with_inline_image_dm(
	http: &serenity::http::Http,
	pacer: &ApiPacer,
	cache: &PostCache,
	command: &CommandInteraction,
	booru: &BooruConfig,
	image: &ImageResult,
	inline_image: InlineImageResult,
) -> Result<serenity::model::channel::Message> {
	let mut builder = EditInteractionResponse::new().new_attachment(CreateAttachment::bytes(
		inline_image.data.clone(),
		inline_image.filename.clone(),
	));

	if let Some(content) = format_inline_image_response(booru, image, inline_image.compressed) {
		builder = builder.content(content);
	}

	pacer.wait().await;

	let cache_id = cache.store(CachedPost {
		image: image.clone(),
		booru_name: booru.name.clone(),
		embed_image: true,
		inline_data: Some(inline_image.data),
		inline_filename: Some(inline_image.filename),
	});

	let button = CreateButton::new(dm_button_id(&cache_id))
		.label("Send to DM")
		.emoji(ReactionType::Unicode("📬".to_string()))
		.style(ButtonStyle::Secondary);

	builder = builder.components(vec![CreateActionRow::Buttons(vec![button])]);

	let message = command
		.edit_response(http, builder)
		.await
		.map_err(anyhow::Error::from)?;
	cli::app_output(
		command.guild_id.map(|g| g.get() as i64),
		command.channel_id.get() as i64,
		"sent_attachment",
	);
	Ok(message)
}

pub(crate) fn log_final_selection(image: &ImageResult, inline_image: &InlineImageResult) {
	if inline_image.compressed {
		cli::final_kept_compressed(
			image.post_url.as_deref(),
			image.upstream_source_url.as_deref(),
			&image.image_url,
			inline_image.compression_time.as_deref().unwrap_or("0ms"),
		);
	} else {
		cli::final_kept(
			image.post_url.as_deref(),
			image.upstream_source_url.as_deref(),
			&image.image_url,
		);
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;

	use crate::config::{BooruConfig, JsonPathSegment};

	use super::*;

	fn booru() -> BooruConfig {
		BooruConfig {
			name: "demo".to_string(),
			embed_image: false,
			max_tags: 2,
			page_size: 100,
			page_base: 1,
			tag_separator: " ".to_string(),
			encode_tag_separator: true,
			tag_spaces_as_plus: false,
			character_space_replacement: "_".to_string(),
			count_url: Some("https://example.test/count".to_string()),
			count_path: vec![],
			posts_url: "https://example.test/posts".to_string(),
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
	fn formats_source_links_before_image_url() {
		let response = format_image_response(
			&booru(),
			&ImageResult {
				image_url: "https://example.test/image.jpg".to_string(),
				post_url: Some("https://example.test/post/1".to_string()),
				upstream_source_url: Some("https://source.example/artworks/123".to_string()),
			},
			false,
		);

		assert_eq!(
			response,
			"[source (demo)](<https://example.test/post/1>) | [source (source.example)](<https://source.example/artworks/123>)\nhttps://example.test/image.jpg"
		);
	}

	#[test]
	fn custom_tag_count_uses_nine_when_unlimited() {
		let mut booru = booru();
		booru.max_tags = 0;
		assert_eq!(custom_tag_count(&booru), 9);
	}
}
