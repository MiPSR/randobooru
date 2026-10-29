use super::{Handler, command_integer_option, command_string_option};
use crate::{cli, config::BooruConfig};
use anyhow::{Context as _, Result, ensure};
use rand::RngExt;
use serenity::all::{
	CommandInteraction, Context, CreateAttachment, CreateInteractionResponse,
	CreateInteractionResponseMessage, CreateMessage, EditInteractionResponse, Interaction, UserId,
};
use std::sync::{
	Arc,
	atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;

pub struct JobTracker {
	active: AtomicUsize,
	accepting: AtomicBool,
	idle: Notify,
	cancelled: AtomicBool,
	cancel: Notify,
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
			cancelled: AtomicBool::new(false),
			cancel: Notify::new(),
		}
	}
	pub fn try_start(self: &Arc<Self>, limit: usize) -> Option<JobGuard> {
		if !self.accepting.load(Ordering::Acquire) {
			return None;
		}
		self.active
			.try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
				(n < limit).then_some(n + 1)
			})
			.ok()?;
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
	pub async fn wait_idle(&self) {
		loop {
			let notified = self.idle.notified();
			tokio::pin!(notified);
			notified.as_mut().enable();
			if self.active.load(Ordering::Acquire) == 0 {
				return;
			}
			notified.await;
		}
	}
	pub fn cancel_all(&self) {
		self.cancelled.store(true, Ordering::Release);
		self.cancel.notify_waiters();
	}
	pub async fn cancelled(&self) {
		let notified = self.cancel.notified();
		tokio::pin!(notified);
		notified.as_mut().enable();
		if !self.cancelled.load(Ordering::Acquire) {
			notified.await;
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

#[derive(Debug)]
struct GuardError(String);
impl std::fmt::Display for GuardError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.0.fmt(f)
	}
}
impl std::error::Error for GuardError {}
fn guard(allowed: bool, message: impl Into<String>) -> Result<()> {
	if allowed {
		Ok(())
	} else {
		Err(GuardError(message.into()).into())
	}
}

pub(crate) async fn handle_interaction(handler: &Handler, ctx: &Context, interaction: Interaction) {
	let mut limit = handler.setting_number("max_concurrent_jobs").unwrap_or(4) as usize;
	if let Interaction::Command(v) = &interaction
		&& v.data.name == "reload"
		&& handler
			.can_administer(v.user.id.get(), v.guild_id.map(|g| g.get() as i64))
			.unwrap_or(false)
	{
		limit = limit.saturating_add(1);
	}
	let Some(_job) = handler.jobs.try_start(limit) else {
		let response = CreateInteractionResponse::Message(
			CreateInteractionResponseMessage::new()
				.content("The bot is busy or reloading. Please try again shortly.")
				.ephemeral(true),
		);
		let result = match &interaction {
			Interaction::Command(v) => v.create_response(&ctx.http, response).await,
			Interaction::Component(v) => v.create_response(&ctx.http, response).await,
			Interaction::Modal(v) => v.create_response(&ctx.http, response).await,
			_ => return,
		};
		if let Err(err) = result {
			cli::error(&err.to_string(), "ERROR");
		}
		return;
	};
	let operation = async {
		match &interaction {
			Interaction::Command(v) => handle_command(handler, ctx, v).await,
			Interaction::Component(v) if v.data.custom_id.starts_with("admin:") => {
				super::admin_embed::handle_component_interaction(handler, v, ctx).await
			}
			Interaction::Modal(v) if v.data.custom_id.starts_with("admin:") => {
				super::admin_embed::handle_modal_interaction(handler, v, ctx).await
			}
			Interaction::Component(v) if v.data.custom_id.starts_with("dm:") => {
				handle_dm_button(handler, v, ctx).await
			}
			_ => Ok(()),
		}
	};
	let result = tokio::select! {
		result=tokio::time::timeout(std::time::Duration::from_secs(120),operation)=>result.unwrap_or_else(|_|Err(anyhow::anyhow!("operation exceeded its 120 second deadline"))),
		_ = handler.jobs.cancelled()=>Err(GuardError("Reload cancelled an unfinished operation. Please retry after reconnecting.".into()).into()),
	};
	if let Err(err) = result {
		let details = format!("{err:#}");
		let expected = err.downcast_ref::<GuardError>().is_some();
		let content = if expected {
			err.to_string()
		} else {
			format!(
				"Operation failed: {}",
				super::truncate_for_dm(&err.to_string(), 1500)
			)
		};
		// Acknowledge failures which occurred before the handler could defer, otherwise edit.
		let response = CreateInteractionResponse::Message(
			CreateInteractionResponseMessage::new()
				.content(&content)
				.ephemeral(true),
		);
		let initial = match &interaction {
			Interaction::Command(v) => v.create_response(&ctx.http, response).await,
			Interaction::Component(v) => v.create_response(&ctx.http, response).await,
			Interaction::Modal(v) => v.create_response(&ctx.http, response).await,
			_ => Ok(()),
		};
		if initial.is_err() {
			let edit = EditInteractionResponse::new().content(&content);
			let edited = match &interaction {
				Interaction::Command(v) => v.edit_response(&ctx.http, edit).await.map(|_| ()),
				Interaction::Component(v) => v.edit_response(&ctx.http, edit).await.map(|_| ()),
				Interaction::Modal(v) => v.edit_response(&ctx.http, edit).await.map(|_| ()),
				_ => Ok(()),
			};
			if let Err(err) = edited {
				cli::error(
					&format!("could not display interaction error: {err}"),
					"ERROR",
				);
			}
		}
		if !expected {
			super::report_error(
				ctx,
				UserId::new(handler.admin_user_id),
				super::ErrorSeverity::Error,
				"interaction",
				"Operation failed",
				Some(&details),
			)
			.await;
		}
	}
}

async fn handle_dm_button(
	handler: &Handler,
	v: &serenity::all::ComponentInteraction,
	ctx: &Context,
) -> Result<()> {
	use super::post_cache::{format_dm_content, parse_dm_button_id};
	v.create_response(
		&ctx.http,
		CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new().ephemeral(true)),
	)
	.await?;
	let id = parse_dm_button_id(&v.data.custom_id).context("invalid DM reference")?;
	let Some(post) = handler.cached_posts.get(id, v.channel_id.get() as i64)? else {
		v.edit_response(
			&ctx.http,
			EditInteractionResponse::new().content("This post is no longer retained in history."),
		)
		.await?;
		return Ok(());
	};
	let mut message = CreateMessage::new()
		.content(format_dm_content(&post))
		.allowed_mentions(serenity::all::CreateAllowedMentions::new());
	if post.embed_image {
		let booru = {
			let db = handler.db.lock().expect("db mutex poisoned");
			BooruConfig::from_row(
				&db.get_booru_by_name(&post.booru_name)?
					.context("provider no longer configured")?,
			)?
		};
		let image = handler
			.fetch_client()?
			.inline_image(&booru, &post.image.image_url)
			.await?;
		message = message.add_file(CreateAttachment::bytes(image.data, image.filename));
	}
	let result = v.user.id.direct_message(&ctx.http, message).await;
	let text = if let Err(err) = result {
		cli::error(&err.to_string(), "DM_ERROR");
		"Could not send the DM. Check that your DMs are open."
	} else {
		"Sent to your DMs."
	};
	v.edit_response(&ctx.http, EditInteractionResponse::new().content(text))
		.await?;
	Ok(())
}

async fn handle_command(
	handler: &Handler,
	ctx: &Context,
	command: &CommandInteraction,
) -> Result<()> {
	let guild = command.guild_id.map(|g| g.get() as i64);
	let channel = command.channel_id.get() as i64;
	let name = command.data.name.as_str();
	let user = command.user.id.get();
	cli::app_input(guild, channel, user as i64, name);
	if name == "administrate" {
		super::admin_embed::handle_administrate_embed(handler, command, ctx, user as i64).await?;
		return handler.reconcile_commands(&ctx.http, false).await;
	}
	if name == "reload" {
		guard(
			handler.can_administer(user, guild)?,
			"reload is restricted to the administrator and guild moderators",
		)?;
		command.defer_ephemeral(&ctx.http).await?;
		request_reload(handler)?;
		command
			.edit_response(
				&ctx.http,
				EditInteractionResponse::new().content(
					"Reload requested. Waiting for active jobs, then closing the gateway before reconnecting.",
				),
			)
			.await?;
		return Ok(());
	}
	if name == "booru-test" {
		guard(
			user == handler.admin_user_id,
			"provider testing is administrator-only",
		)?;
		command.defer_ephemeral(&ctx.http).await?;
		let booru = command_string_option(command, "booru").context("provider is required")?;
		let tags = command_string_option(command, "tags").context("tags are required")?;
		let row = {
			handler
				.db
				.lock()
				.expect("db mutex poisoned")
				.get_booru_by_name(&booru)?
				.context("provider not found")?
		};
		return test_booru(handler, ctx, command, &BooruConfig::from_row(&row)?, &tags).await;
	}
	let i18n = handler.channel_i18n(guild, channel);
	let privileged = handler.can_administer(user, guild)?;
	let Some(guild_id) = guild else {
		handler
			.respond(&ctx.http, command, "Use this command in a server.")
			.await;
		return Ok(());
	};
	let (validated, allowed) = {
		let db = handler.db.lock().expect("db mutex poisoned");
		(
			db.get_server(guild_id)?.is_some_and(|s| s.validated),
			db.has_channel(guild_id, channel)?,
		)
	};
	if !validated {
		handler
			.respond(&ctx.http, command, i18n.server_not_validated())
			.await;
		return Ok(());
	}
	if !allowed && !privileged {
		handler
			.respond(&ctx.http, command, i18n.channel_not_allowed())
			.await;
		return Ok(());
	}
	if name == "tag-test" {
		guard(
			privileged,
			"tag testing is restricted to guild moderators and the administrator",
		)?;
		command.defer_ephemeral(&ctx.http).await?;
	} else {
		command.defer(&ctx.http).await?;
	}
	handler.reconcile_commands(&ctx.http, false).await?;
	if name == "art-history" {
		return handle_art_history(handler, ctx, command).await;
	}
	let pattern = if name == "tag-test" {
		command_string_option(command, "tag").context("tag name is required")?
	} else {
		name.to_string()
	};
	let candidates = candidates(handler, command, &pattern)?;
	let Some(candidates) = candidates else {
		handler
			.edit_deferred(&ctx.http, command, i18n.command_not_registered())
			.await;
		super::report_error(
			ctx,
			UserId::new(handler.admin_user_id),
			super::ErrorSeverity::Bug,
			"unknown slash command",
			&format!("name={name} guild={guild_id} channel={channel} user={user}"),
			None,
		)
		.await;
		return Ok(());
	};
	if candidates.is_empty() {
		return Ok(());
	}
	let attempts = handler.setting_number("booru_fetch_retry_limit")? as usize + 1;
	let mut error = anyhow::anyhow!("no eligible image found");
	for attempt in 1..=attempts {
		let index = rand::rng().random_range(0..candidates.len());
		let (booru, tags, blacklist) = &candidates[index];
		match fetch_and_deliver(
			handler,
			ctx,
			command,
			booru,
			tags,
			blacklist,
			name == "tag-test",
		)
		.await
		{
			Ok(()) => return Ok(()),
			Err(err) => {
				error = err;
				cli::final_retry(&error.to_string(), attempt);
				if attempt < attempts {
					tokio::time::sleep(std::time::Duration::from_millis(
						250 * attempt.min(8) as u64,
					))
					.await;
				}
			}
		}
	}
	command
		.edit_response(
			&ctx.http,
			EditInteractionResponse::new().content(i18n.could_not_find_image(&error.to_string())),
		)
		.await?;
	Err(error)
}

type Candidate = (BooruConfig, Vec<String>, Vec<String>);
fn candidates(
	handler: &Handler,
	command: &CommandInteraction,
	name: &str,
) -> Result<Option<Vec<Candidate>>> {
	let guild = command.guild_id.context("guild is required")?.get() as i64;
	let channel = command.channel_id.get() as i64;
	let mut blacklist = handler.blacklist(Some(guild), channel)?;
	let i18n = handler.channel_i18n(Some(guild), channel);
	let db = handler.db.lock().expect("db mutex poisoned");
	for row in db.get_enabled_boorus()? {
		if super::custom_command_name(&row.name) == name {
			let booru = BooruConfig::from_row(&row)?;
			let tags = (1..=super::responses::custom_tag_count(&booru))
				.filter_map(|i| command_string_option(command, &format!("tag_{i}")))
				.map(|t| {
					super::responses::normalize_user_tag(&t, &booru.character_space_replacement)
				})
				.filter(|t| !t.is_empty())
				.collect::<Vec<_>>();
			guard(!tags.is_empty(), i18n.custom_command_no_tags())?;
			ensure!(
				tags.iter().all(|t| t.chars().count() <= 256)
					&& tags.iter().map(String::len).sum::<usize>() <= 4096,
				"tag query is too long"
			);
			return Ok(Some(vec![(booru, tags, blacklist)]));
		}
	}
	let Some(pattern) = db
		.get_unique_pattern_names()?
		.into_iter()
		.find(|p| p == name || super::discord_name_component(p) == name)
	else {
		return Ok(None);
	};
	// Never bypass the server whitelist: moderators are guild-scoped too.
	guard(
		db.get_server_tags(guild)?.contains(&pattern),
		i18n.tag_not_registered(&pattern),
	)?;
	guard(
		!db.get_channel_patterns(guild, channel)?.contains(&pattern),
		i18n.channel_tag_blocked(&pattern),
	)?;
	let mut candidates = Vec::new();
	for p in db.get_tag_patterns(Some(&pattern))? {
		let Some(row) = db.get_booru_by_id(p.booru_id)?.filter(|b| b.enabled) else {
			continue;
		};
		let mut tags = Vec::new();
		let base = blacklist.len();
		for entry in db.get_pattern_entries(p.id)? {
			if entry.is_excluded {
				blacklist.push(entry.tag);
			} else {
				tags.push(entry.tag);
			}
		}
		if !tags.is_empty() {
			candidates.push((BooruConfig::from_row(&row)?, tags, blacklist.clone()));
		}
		blacklist.truncate(base);
	}
	ensure!(
		!candidates.is_empty(),
		"no enabled provider for this pattern"
	);
	Ok(Some(candidates))
}

async fn fetch_and_deliver(
	handler: &Handler,
	ctx: &Context,
	command: &CommandInteraction,
	booru: &BooruConfig,
	tags: &[String],
	blacklist: &[String],
	test: bool,
) -> Result<()> {
	let channel = command.channel_id.get() as i64;
	let history = |source: &str| {
		handler
			.db
			.lock()
			.expect("db mutex poisoned")
			.art_history_exists(source, channel)
			.unwrap_or(true)
	};
	let client = handler.fetch_client()?;
	let image = client
		.random_image(
			booru,
			tags,
			blacklist,
			if test { None } else { Some(&history) },
		)
		.await?;
	// Serialize only the final deduplication/delivery/commit, never hold a DB lock over await.
	let lock = handler.delivery_lock(channel);
	let _delivery = lock.lock().await;
	if !test && let Some(source) = image.history_source() {
		ensure!(
			!handler
				.db
				.lock()
				.expect("db mutex poisoned")
				.art_history_exists(source, channel)?,
			"post was already delivered in this channel"
		);
	}
	let reference = handler
		.send_image_response(&ctx.http, command, booru, &image)
		.await?;
	if !test && let Some(source) = image.history_source() {
		let db = handler.db.lock().expect("db mutex poisoned");
		ensure!(
			db.register_art(
				source,
				channel,
				command.guild_id.map(|g| g.get() as i64),
				Some(&booru.name)
			)?,
			"history changed during delivery"
		);
		db.link_delivered_post(&reference, source, channel)?;
	}
	Ok(())
}

pub(crate) async fn test_booru(
	handler: &Handler,
	ctx: &Context,
	command: &CommandInteraction,
	booru: &BooruConfig,
	input: &str,
) -> Result<()> {
	let tags = test_tags(input, booru)?;
	let blacklist = handler.blacklist(
		command.guild_id.map(|g| g.get() as i64),
		command.channel_id.get() as i64,
	)?;
	fetch_and_deliver(handler, ctx, command, booru, &tags, &blacklist, true).await
}

pub(crate) fn test_tags(input: &str, booru: &BooruConfig) -> Result<Vec<String>> {
	let tags = input
		.split(',')
		.map(|tag| super::responses::normalize_user_tag(tag, &booru.character_space_replacement))
		.filter(|t| !t.is_empty())
		.collect::<Vec<_>>();
	ensure!(
		!tags.is_empty() && (booru.max_tags == 0 || tags.len() <= booru.max_tags),
		"invalid number of test tags"
	);
	Ok(tags)
}

pub(crate) fn request_reload(handler: &Handler) -> Result<()> {
	ensure!(handler.jobs.begin_reload(), "reload already in progress");
	handler.reload_requested.store(true, Ordering::Release);
	handler.reload_notify.notify_one();
	Ok(())
}

async fn handle_art_history(
	handler: &Handler,
	ctx: &Context,
	command: &CommandInteraction,
) -> Result<()> {
	let count = command_integer_option(command, "previous_arts")
		.unwrap_or(10)
		.clamp(1, 100_000) as usize;
	let entries = {
		handler
			.db
			.lock()
			.expect("db mutex poisoned")
			.recent_art(command.channel_id.get() as i64, count)?
	};
	let links = entries
		.into_iter()
		.map(|e| e.source_link)
		.collect::<Vec<_>>();
	let i18n = handler.channel_i18n(
		command.guild_id.map(|g| g.get() as i64),
		command.channel_id.get() as i64,
	);
	let summary = if links.is_empty() {
		i18n.art_history_no_links().to_string()
	} else if links.len() < count {
		i18n.art_history_showing_all(count, links.len())
	} else {
		i18n.art_history_showing_count(links.len())
	};
	let body = format!(
		"{summary}\n{}",
		super::responses::format_recent_art_links(&links)
	);
	let mut response = EditInteractionResponse::new();
	if body.chars().count() <= 1900 {
		response = response.content(body);
	} else {
		response = response
			.content(summary)
			.new_attachment(CreateAttachment::bytes(
				body.into_bytes(),
				i18n.art_history_attachment_filename(),
			));
	}
	command.edit_response(&ctx.http, response).await?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[tokio::test]
	async fn jobs_are_bounded_and_reload_waits() {
		let jobs = Arc::new(JobTracker::new());
		let guard = jobs.try_start(1).unwrap();
		assert!(jobs.try_start(1).is_none());
		assert!(jobs.begin_reload());
		assert!(!jobs.begin_reload());
		assert!(jobs.try_start(10).is_none());
		drop(guard);
		tokio::time::timeout(std::time::Duration::from_secs(1), jobs.wait_idle())
			.await
			.unwrap();
	}
	#[tokio::test]
	async fn reload_notification_is_not_lost() {
		let notify = Notify::new();
		notify.notify_one();
		tokio::time::timeout(std::time::Duration::from_secs(1), notify.notified())
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn cancellation_is_observed_even_before_polling() {
		let jobs = Arc::new(JobTracker::new());
		let guard = jobs.try_start(1).unwrap();
		jobs.begin_reload();
		jobs.cancel_all();
		tokio::time::timeout(std::time::Duration::from_secs(1), jobs.cancelled())
			.await
			.unwrap();
		drop(guard);
		tokio::time::timeout(std::time::Duration::from_secs(1), jobs.wait_idle())
			.await
			.unwrap();
	}
}
