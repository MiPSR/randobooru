use std::{
	collections::HashMap,
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicUsize, Ordering},
	},
};

use rand::RngExt;
use serenity::all::{
	CommandInteraction, Context, CreateInteractionResponse, CreateInteractionResponseMessage,
	EditInteractionResponse, Interaction, UserId,
};
use tokio::sync::Notify;

use crate::{cli, config::BooruConfig, i18n::I18n};

use super::{Handler, command_integer_option, command_string_option};

pub struct JobTracker {
	active: AtomicUsize,
	accepting: AtomicBool,
	idle: Notify,
}

pub struct JobGuard {
	jobs: Arc<JobTracker>,
}

impl JobTracker {
	pub fn new() -> Self {
		Self {
			active: AtomicUsize::new(0),
			accepting: AtomicBool::new(true),
			idle: Notify::new(),
		}
	}

	pub fn try_start(self: &Arc<Self>) -> Option<JobGuard> {
		if !self.accepting.load(Ordering::Acquire) {
			return None;
		}

		self.active.fetch_add(1, Ordering::AcqRel);
		if self.accepting.load(Ordering::Acquire) {
			Some(JobGuard {
				jobs: Arc::clone(self),
			})
		} else {
			self.finish();
			None
		}
	}

	pub fn begin_reload(&self) -> bool {
		self.accepting.swap(false, Ordering::AcqRel)
	}

	pub fn active_count(&self) -> usize {
		self.active.load(Ordering::Acquire)
	}

	pub async fn wait_idle(&self) {
		while self.active.load(Ordering::Acquire) != 0 {
			self.idle.notified().await;
		}
	}

	fn finish(&self) {
		if self.active.fetch_sub(1, Ordering::AcqRel) == 1 {
			self.idle.notify_waiters();
		}
	}
}

impl Drop for JobGuard {
	fn drop(&mut self) {
		self.jobs.finish();
	}
}

pub(crate) async fn handle_interaction(handler: &Handler, ctx: &Context, interaction: Interaction) {
	let owner = UserId::new(handler.admin_user_id);
	let result: anyhow::Result<()> = match &interaction {
		Interaction::Command(command) => handle_command(handler, ctx, command).await,
		Interaction::Component(component)
			if (component.data.custom_id.starts_with("ac:")
				|| component.data.custom_id.starts_with("aa:")
				|| component.data.custom_id.starts_with("as:")
				|| component.data.custom_id.starts_with("ay:")
				|| component.data.custom_id == "an") =>
		{
			super::admin_embed::handle_component_interaction(handler, component, ctx).await;
			Ok(())
		}
		Interaction::Component(component) if component.data.custom_id.starts_with("dm:") => {
			handle_dm_button(handler, component, ctx).await;
			Ok(())
		}
		Interaction::Modal(modal) if modal.data.custom_id.starts_with("am:") => {
			super::admin_embed::handle_modal_interaction(handler, modal, ctx).await;
			Ok(())
		}
		_ => Ok(()),
	};
	if let Err(err) = result {
		super::report_error(
			ctx,
			owner,
			super::ErrorSeverity::Error,
			"interaction_create",
			&err.to_string(),
			None,
		)
		.await;
	}
}

async fn handle_dm_button(
	handler: &Handler,
	component: &serenity::all::ComponentInteraction,
	ctx: &Context,
) {
	use crate::app::post_cache::{format_dm_content, parse_dm_button_id};

	let custom_id = &component.data.custom_id;
	let Some(cache_id) = parse_dm_button_id(custom_id) else {
		return;
	};

	let user_id = component.user.id;

	let Some(post) = handler.cached_posts.get(cache_id) else {
		let _ = component
			.create_response(
				&ctx.http,
				CreateInteractionResponse::Message(
					CreateInteractionResponseMessage::new()
						.content("This post is no longer available. Please request a new one.")
						.ephemeral(true),
				),
			)
			.await;
		return;
	};

	let mut builder = serenity::all::CreateMessage::new().content(format_dm_content(&post));

	if let (Some(data), Some(filename)) = (&post.inline_data, &post.inline_filename) {
		builder = builder.add_file(serenity::all::CreateAttachment::bytes(
			data.clone(),
			filename.clone(),
		));
	}

	let dm_result = user_id.direct_message(&ctx.http, builder).await;

	let response_content = match dm_result {
		Ok(_) => "Sent to your DMs.",
		Err(err) => {
			cli::error(&err.to_string(), "dm_error");
			"Failed to send DM. Make sure your DMs are open."
		}
	};

	let _ = component
		.create_response(
			&ctx.http,
			CreateInteractionResponse::Message(
				CreateInteractionResponseMessage::new()
					.content(response_content)
					.ephemeral(true),
			),
		)
		.await;
}

async fn handle_command(
	handler: &Handler,
	ctx: &Context,
	command: &serenity::all::CommandInteraction,
) -> anyhow::Result<()> {
	let guild_id = command.guild_id.map(|g| g.get() as i64);
	let channel_id = command.channel_id.get() as i64;
	let user_id = command.user.id.get() as i64;
	let command_name = command.data.name.as_str();
	cli::app_input(guild_id, channel_id, user_id, command_name);

	let result: anyhow::Result<()> = async {
		if let Some(guild_id) = guild_id {
			let (server_validated, channel_allowed) = {
				let db = handler.db.lock().expect("db mutex poisoned");
				let server = db.get_server(guild_id).ok().flatten();
				let validated = server.map(|s| s.validated).unwrap_or(false);
				if !validated {
					(false, false)
				} else {
					(true, db.has_channel(guild_id, channel_id).unwrap_or(false))
				}
			};

			if !server_validated {
				if command_name == "administrate"
					&& (user_id == handler.admin_user_id as i64 || {
						let db = handler.db.lock().expect("db mutex poisoned");
						db.is_moderator(user_id, Some(guild_id)).unwrap_or(false)
					}) {
					super::admin_embed::handle_administrate_embed(
						handler, command, ctx, user_id,
					)
					.await;
					return Ok(());
				}

				handler
					.respond(&ctx.http, command, handler.i18n.server_not_validated())
					.await;
				return Ok(());
			}

			if !channel_allowed {
				let is_admin = user_id == handler.admin_user_id as i64;
				let is_mod = {
					let db = handler.db.lock().expect("db mutex poisoned");
					db.is_moderator(user_id, Some(guild_id)).unwrap_or(false)
				};
				if !is_admin && !is_mod {
					handler
						.respond(&ctx.http, command, handler.i18n.channel_not_allowed())
						.await;
					return Ok(());
				}
			}
		}

		let i18n = handler.channel_i18n(guild_id, channel_id);

		match command_name {
			"art-history" => {
				handle_art_history(handler, ctx, command, &i18n).await;
				return Ok(());
			}
			"administrate" => {
				super::admin_embed::handle_administrate_embed(handler, command, ctx, user_id)
					.await;
				return Ok(());
			}
			_ => {}
		}

		let Some(_job) = handler.jobs.try_start() else {
			handler
				.respond(&ctx.http, command, i18n.reload_toml_in_progress())
				.await;
			return Ok(());
		};

		if command.defer(&ctx.http).await.is_err() {
			return Ok(());
		}

		if let Some(result) =
			handle_custom_command(handler, ctx, command, command_name, &i18n).await
		{
			match result {
				Ok(()) => return Ok(()),
				Err(err) => {
					handler
						.edit_deferred(
							&ctx.http,
							command,
							i18n.could_not_find_image(&err.to_string()),
						)
						.await;
					return Ok(());
				}
			}
		}

		if let Some(result) = handle_pattern_command(
			handler,
			ctx,
			command,
			command_name,
			channel_id,
			guild_id,
			&i18n,
		)
		.await
		{
			match result {
				Ok(()) => return Ok(()),
				Err(err) => match err_tag_blocked(&err) {
					Some(tag) => {
						handler
							.edit_deferred(&ctx.http, command, i18n.channel_tag_blocked(&tag))
							.await;
						return Ok(());
					}
					None => {
						handler
							.edit_deferred(
								&ctx.http,
								command,
								i18n.could_not_find_image(&err.to_string()),
							)
							.await;
						return Ok(());
					}
				},
			}
		}

		handler
			.edit_deferred(&ctx.http, command, i18n.command_not_registered())
			.await;
		super::report_error(
			ctx,
			UserId::new(handler.admin_user_id),
			super::ErrorSeverity::Bug,
			"command_not_registered",
			&format!(
				"unknown slash command `{command_name}` invoked by user {user_id} in guild {guild_id:?} channel {channel_id}"
			),
			None,
		)
		.await;
		Ok(())
	}
	.await;

	if let Err(err) = &result {
		super::report_error(
			ctx,
			UserId::new(handler.admin_user_id),
			super::ErrorSeverity::Error,
			"handle_command",
			&format!("command={command_name} user={user_id} guild={guild_id:?}"),
			Some(&err.to_string()),
		)
		.await;
	}
	result
}

fn err_tag_blocked(err: &anyhow::Error) -> Option<String> {
	err.downcast_ref::<TagBlockedError>().map(|e| e.tag.clone())
}

#[derive(Debug)]
struct TagBlockedError {
	tag: String,
}

impl std::fmt::Display for TagBlockedError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "tag `{}` is blocked in this channel", self.tag)
	}
}

async fn handle_art_history(
	handler: &Handler,
	ctx: &Context,
	command: &CommandInteraction,
	i18n: &I18n,
) {
	let channel_id = command.channel_id.get() as i64;

	handler.respond(&ctx.http, command, "...").await;

	let requested = command_integer_option(command, "previous_arts")
		.and_then(|value| usize::try_from(value).ok())
		.unwrap_or(10);

	let links_result = {
		let db = handler.db.lock().expect("db mutex poisoned");
		db.recent_art(channel_id, requested)
			.map(|entries| {
				let links: Vec<String> = entries
					.iter()
					.map(|e| {
						let mut label = format!("[ch:{}]", e.channel_id);
						if let Some(booru) = e.booru_name.as_deref() {
							label.push_str(&format!(" [{}]", booru));
						}
						if let Some(guild_id) = e.guild_id {
							label.push_str(&format!(" (guild:{})", guild_id));
						}
						label.push_str(&format!(" <{}>", e.source_link));
						label
					})
					.collect();
				(requested, links)
			})
			.map_err(|err| i18n.art_history_error(&err.to_string()))
	};

	let (requested, links) = match links_result {
		Ok(data) => data,
		Err(error_msg) => {
			handler.edit_deferred(&ctx.http, command, error_msg).await;
			return;
		}
	};

	let shown = links.len();
	let summary = if shown == 0 {
		i18n.art_history_no_links().to_string()
	} else if shown < requested {
		i18n.art_history_showing_all(requested, shown)
	} else {
		i18n.art_history_showing_count(shown)
	};

	let body = if links.is_empty() {
		summary.clone()
	} else {
		format!(
			"{}\n{}",
			summary,
			super::responses::format_recent_art_links(&links)
		)
	};

	if body.len() <= 1900 {
		handler.edit_deferred(&ctx.http, command, body).await;
	} else {
		let attachment = serenity::all::CreateAttachment::bytes(
			body.into_bytes(),
			i18n.art_history_attachment_filename(),
		);
		handler.pacer.wait().await;
		if command
			.edit_response(
				&ctx.http,
				EditInteractionResponse::new()
					.content(summary.clone())
					.new_attachment(attachment),
			)
			.await
			.is_ok()
		{
			cli::app_output(
				command.guild_id.map(|g| g.get() as i64),
				command.channel_id.get() as i64,
				"sent_attachment",
			);
		}
	}
}

async fn handle_custom_command(
	handler: &Handler,
	ctx: &Context,
	command: &CommandInteraction,
	command_name: &str,
	i18n: &I18n,
) -> Option<Result<(), anyhow::Error>> {
	use super::responses::{custom_command_name, custom_tag_count, normalize_user_tag};

	command_name.strip_suffix("-custom")?;

	let (booru, tags) = {
		let db = handler.db.lock().expect("db mutex poisoned");
		let booru_row = match db.get_enabled_boorus() {
			Ok(boorus) => boorus
				.into_iter()
				.find(|booru| custom_command_name(&booru.name) == command_name)?,
			Err(err) => return Some(Err(err)),
		};

		let booru = match BooruConfig::from_row(&booru_row) {
			Ok(booru) => booru,
			Err(err) => return Some(Err(err)),
		};

		let tags: Vec<String> = (1..=custom_tag_count(&booru))
			.filter_map(|i| {
				let name = format!("tag_{i}");
				command_string_option(command, &name)
			})
			.filter(|t| !t.trim().is_empty())
			.map(|t| normalize_user_tag(&t, &booru.character_space_replacement))
			.collect();

		if tags.is_empty() {
			return Some(Err(anyhow::anyhow!(i18n.custom_command_no_tags())));
		}

		(booru, tags)
	};

	let mut blacklist = handler.booru_tag_blacklist.to_vec();
	if let Some(guild_id) = command.guild_id.map(|g| g.get() as i64)
		&& let Ok(Some(cfg)) = handler
			.db
			.lock()
			.expect("db mutex poisoned")
			.get_channel_config(guild_id, command.channel_id.get() as i64)
	{
		blacklist.extend(cfg.banned_tags.iter().cloned());
	}
	let max_attempts = handler.booru_fetch_retry_limit.saturating_add(1);
	let candidates = vec![(booru, tags, blacklist)];

	for attempt in 1..=max_attempts {
		match run_with_retry(handler, ctx, command, &candidates, attempt, max_attempts).await {
			RetryOutcome::Sent => return Some(Ok(())),
			RetryOutcome::Exhausted(err) => {
				handler
					.respond(
						&ctx.http,
						command,
						handler.i18n.could_not_find_image(&err.to_string()),
					)
					.await;
				return Some(Ok(()));
			}
			RetryOutcome::Continue => {}
		}
	}

	Some(Ok(()))
}

async fn handle_pattern_command(
	handler: &Handler,
	ctx: &Context,
	command: &CommandInteraction,
	pattern_name: &str,
	channel_id: i64,
	guild_id: Option<i64>,
	i18n: &I18n,
) -> Option<Result<(), anyhow::Error>> {
	use super::responses::discord_name_component;

	let guild_id = guild_id?;
	let pattern_name = {
		let db = handler.db.lock().expect("db mutex poisoned");
		match db.get_unique_pattern_names() {
			Ok(names) => names
				.into_iter()
				.find(|name| discord_name_component(name) == pattern_name)?,
			Err(err) => return Some(Err(err)),
		}
	};
	let pattern_name = pattern_name.as_str();

	let is_admin = command.user.id.get() == handler.admin_user_id;
	let is_mod = {
		let db = handler.db.lock().expect("db mutex poisoned");
		db.is_moderator(command.user.id.get() as i64, Some(guild_id))
			.unwrap_or(false)
	};

	let (allowed_by_server, channel_cfg, blocked_in_channel) = {
		let db = handler.db.lock().expect("db mutex poisoned");
		let allowed = db
			.get_server_tags(guild_id)
			.ok()
			.map(|names| names.iter().any(|n| n == pattern_name))
			.unwrap_or(false);
		let cfg = db.get_channel_config(guild_id, channel_id).ok().flatten();
		let blocked = db
			.get_channel_patterns(guild_id, channel_id)
			.ok()
			.map(|names| names.iter().any(|n| n == pattern_name))
			.unwrap_or(false);
		(allowed, cfg, blocked)
	};

	if !allowed_by_server && !is_admin && !is_mod {
		return Some(Err(anyhow::anyhow!(i18n.tag_not_registered(pattern_name))));
	}
	if blocked_in_channel {
		return Some(Err(anyhow::anyhow!(TagBlockedError {
			tag: pattern_name.to_string(),
		})));
	}
	let (enabled_booru_ids, pattern_entries) = {
		let db = handler.db.lock().expect("db mutex poisoned");
		let booru_ids = db.get_booru_ids_for_pattern(pattern_name).ok()?;
		if booru_ids.is_empty() {
			return None;
		}

		let enabled_booru_ids: Vec<i64> = booru_ids
			.iter()
			.filter_map(|&id| {
				let row = db.get_booru_by_id(id).ok()??;
				if row.enabled { Some(id) } else { None }
			})
			.collect();

		if enabled_booru_ids.is_empty() {
			return None;
		}

		let pattern_entries: HashMap<i64, (Vec<String>, Vec<String>)> = enabled_booru_ids
			.iter()
			.filter_map(|&booru_id| {
				let patterns = db.get_tag_patterns(Some(pattern_name)).ok()?;
				let pattern = patterns.iter().find(|p| p.booru_id == booru_id)?;
				let entries = db.get_pattern_entries(pattern.id).ok()?;
				let included: Vec<String> = entries
					.iter()
					.filter(|e| !e.is_excluded)
					.map(|e| e.tag.clone())
					.collect();
				let excluded: Vec<String> = entries
					.iter()
					.filter(|e| e.is_excluded)
					.map(|e| e.tag.clone())
					.collect();
				if included.is_empty() {
					None
				} else {
					Some((booru_id, (included, excluded)))
				}
			})
			.collect();

		if pattern_entries.is_empty() {
			return None;
		}

		(enabled_booru_ids, pattern_entries)
	};

	let max_attempts = handler.booru_fetch_retry_limit.saturating_add(1);

	for attempt in 1..=max_attempts {
		let selected_booru_id = {
			let ids: Vec<i64> = enabled_booru_ids.clone();
			if ids.is_empty() {
				return None;
			}
			let index = rand::rng().random_range(0..ids.len());
			cli::booru_random("booru", index);
			ids[index]
		};

		let (included, excluded) = match pattern_entries.get(&selected_booru_id) {
			Some(v) => v.clone(),
			None => continue,
		};

		let booru_row = {
			let db = handler.db.lock().expect("db mutex poisoned");
			match db.get_booru_by_id(selected_booru_id) {
				Ok(Some(row)) => row,
				_ => continue,
			}
		};

		let booru = match BooruConfig::from_row(&booru_row) {
			Ok(b) => b,
			Err(_) => continue,
		};

		let mut blacklist: Vec<String> = handler.booru_tag_blacklist.to_vec();
		blacklist.extend(excluded.iter().cloned());
		if let Some(ref cfg) = channel_cfg {
			blacklist.extend(cfg.banned_tags.iter().cloned());
		}

		let candidates = vec![(booru, included, blacklist)];
		match run_with_retry(handler, ctx, command, &candidates, attempt, max_attempts).await {
			RetryOutcome::Sent => return Some(Ok(())),
			RetryOutcome::Exhausted(err) => return Some(Err(err)),
			RetryOutcome::Continue => {}
		}
	}

	Some(Err(anyhow::anyhow!("too many retries")))
}

enum RetryOutcome {
	Sent,
	Exhausted(anyhow::Error),
	Continue,
}

async fn run_with_retry(
	handler: &Handler,
	ctx: &Context,
	command: &CommandInteraction,
	candidates: &[(BooruConfig, Vec<String>, Vec<String>)],
	attempt: usize,
	max_attempts: usize,
) -> RetryOutcome {
	let channel_id = command.channel_id.get() as i64;
	let (booru, tags, blacklist) = match candidates.first() {
		Some(entry) => entry,
		None => return RetryOutcome::Exhausted(anyhow::anyhow!("no candidates")),
	};

	match handler.booru.random_image(booru, tags, blacklist).await {
		Ok(image) => {
			if let Some(source_link) = image.post_url.as_deref() {
				let already_sent = {
					let db = handler.db.lock().expect("db mutex poisoned");
					db.art_history_exists(source_link, channel_id)
						.unwrap_or(false)
				};

				if already_sent {
					cli::final_retry("source_link_already_in_art_history", attempt);
					handler.pacer.wait().await;
					return RetryOutcome::Continue;
				}

				{
					let db = handler.db.lock().expect("db mutex poisoned");
					let _ = db.register_art(
						source_link,
						channel_id,
						command.guild_id.map(|g| g.get() as i64),
						Some(&booru.name),
					);
				}
			}

			let _ = handler
				.send_image_response(&ctx.http, command, booru, &image)
				.await;
			RetryOutcome::Sent
		}
		Err(err) => {
			if attempt >= max_attempts {
				RetryOutcome::Exhausted(err)
			} else {
				cli::final_retry(&err.to_string(), attempt);
				handler.pacer.wait().await;
				RetryOutcome::Continue
			}
		}
	}
}
