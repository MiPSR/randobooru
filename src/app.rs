use std::{
	collections::HashMap,
	env,
	sync::{
		Arc, Mutex, Weak,
		atomic::{AtomicBool, Ordering},
	},
	time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use serenity::{
	all::{
		ApplicationId, CommandInteraction, CommandOptionType, Context, CreateCommand,
		CreateCommandOption, CreateMessage, EventHandler, GatewayIntents, Interaction, Ready,
		ResolvedValue, UserId,
	},
	async_trait,
	http::GuildPagination,
};
use tokio::sync::Notify;

use crate::{
	booru::{BooruClient, ImageResult},
	cli,
	config::{BooruConfig, RuntimeValues},
	db::Database,
	i18n::I18n,
	pacing::ApiPacer,
};

mod admin;
mod admin_embed;
mod commands;
pub(crate) mod post_cache;
mod responses;
mod secrets;

pub(crate) use commands::JobTracker;
pub(crate) use post_cache::PostCache;
pub(crate) use responses::{custom_command_name, discord_name_component};
pub(crate) use secrets::SecretConfig;

pub struct Handler {
	db: Arc<Mutex<Database>>,
	booru: BooruClient,
	pacer: ApiPacer,
	i18n: Arc<I18n>,
	i18ns: HashMap<String, I18n>,
	admin_user_id: u64,
	jobs: Arc<JobTracker>,
	reload_requested: Arc<AtomicBool>,
	reload_notify: Arc<Notify>,
	cached_posts: Arc<PostCache>,
	admin_state: admin_embed::PanelStore,
	command_sync: tokio::sync::Mutex<()>,
	last_command_sync: Mutex<Instant>,
	delivery_locks: Mutex<HashMap<i64, Weak<tokio::sync::Mutex<()>>>>,
	discord_token: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ErrorSeverity {
	Error,
	Bug,
}

impl ErrorSeverity {
	fn label(self) -> &'static str {
		match self {
			ErrorSeverity::Error => "ERROR",
			ErrorSeverity::Bug => "BUG",
		}
	}
}

pub(crate) async fn report_error(
	ctx: &Context,
	owner: UserId,
	severity: ErrorSeverity,
	context: &str,
	summary: &str,
	details: Option<&str>,
) {
	cli::error(summary, severity.label());
	let mut body = format!(
		"**{label}** in `{ctx}`\n{summary}",
		label = severity.label(),
		ctx = truncate_for_dm(context, 200),
		summary = truncate_for_dm(summary, 1500),
	);
	if let Some(details) = details {
		body.push_str("\n```\n");
		body.push_str(&truncate_for_dm(details, 1000));
		body.push_str("\n```");
	}
	body = truncate_for_dm(&body, 1900);
	let delivered = tokio::time::timeout(
		Duration::from_secs(10),
		owner.direct_message(
			&ctx.http,
			CreateMessage::new()
				.content(body)
				.allowed_mentions(serenity::all::CreateAllowedMentions::new()),
		),
	)
	.await;
	match delivered {
		Ok(Ok(_)) => {}
		Ok(Err(err)) => cli::error(&format!("owner error report failed: {err}"), "ERROR"),
		Err(_) => cli::error("owner error report timed out", "ERROR"),
	}
}

fn truncate_for_dm(value: &str, max: usize) -> String {
	if value.chars().count() <= max {
		value.to_string()
	} else {
		let mut out: String = value.chars().take(max.saturating_sub(3)).collect();
		out.push_str("...");
		out
	}
}

pub(crate) fn command_string_option(command: &CommandInteraction, name: &str) -> Option<String> {
	command
		.data
		.options()
		.into_iter()
		.find(|option| option.name == name)
		.and_then(|option| match option.value {
			ResolvedValue::String(value) => Some(value.to_string()),
			_ => None,
		})
}

pub(crate) fn command_integer_option(command: &CommandInteraction, name: &str) -> Option<i64> {
	command
		.data
		.options()
		.into_iter()
		.find(|option| option.name == name)
		.and_then(|option| match option.value {
			ResolvedValue::Integer(value) => Some(value),
			_ => None,
		})
}

#[async_trait]
impl EventHandler for Handler {
	async fn ready(&self, _ctx: Context, _ready: Ready) {
		cli::bot_loaded();
	}

	async fn guild_create(&self, ctx: Context, guild: serenity::all::Guild, _is_new: Option<bool>) {
		let Some(_job) = self
			.jobs
			.try_start(self.setting_number("max_concurrent_jobs").unwrap_or(4) as usize)
		else {
			return;
		};
		let guild_id = guild.id.get() as i64;
		let name = guild.name.clone();
		let result = {
			let db = self.db.lock().expect("db mutex poisoned");
			let new = db.get_server(guild_id).map(|s| s.is_none());
			db.add_server(guild_id, &name).and(new)
		};
		match result {
			Err(err) => cli::error(&format!("{err:#}"), "ERROR"),
			Ok(true) => {
				if let Err(err) = self.reconcile_commands(&ctx.http, true).await {
					cli::error(&format!("{err:#}"), "ERROR");
				}
			}
			Ok(false) => {}
		}
	}

	async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
		commands::handle_interaction(self, &ctx, interaction).await;
	}
}

impl Handler {
	pub(crate) fn can_administer(&self, user: u64, guild: Option<i64>) -> Result<bool> {
		Ok(user == self.admin_user_id
			|| self
				.db
				.lock()
				.expect("db mutex poisoned")
				.is_moderator(user as i64, guild)?)
	}
	pub(crate) fn setting_number(&self, key: &str) -> Result<u64> {
		crate::settings::number(&self.db.lock().expect("db mutex poisoned"), key)
	}
	pub(crate) fn discord_pacer(&self) -> Result<ApiPacer> {
		Ok(self.pacer.with_interval(Duration::from_millis(
			self.setting_number("discord_api_pace_ms")?,
		)))
	}
	pub(crate) fn fetch_client(&self) -> Result<BooruClient> {
		let db = self.db.lock().expect("db mutex poisoned");
		let mut runtime = RuntimeValues {
			secrets: HashMap::from([("discord_token".into(), self.discord_token.clone())]),
			settings: db.get_all_settings()?.into_iter().collect(),
			custom: HashMap::new(),
		};
		for (key, _) in crate::settings::DEFAULTS {
			runtime
				.settings
				.insert(key.to_string(), crate::settings::get(&db, key)?);
		}
		for param in db.get_all_booru_custom_parameters()? {
			runtime
				.custom
				.entry(param.booru_name)
				.or_default()
				.insert(param.key, param.value);
		}
		Ok(self.booru.with_runtime(runtime))
	}
	pub(crate) fn blacklist(&self, guild: Option<i64>, channel: i64) -> Result<Vec<String>> {
		let db = self.db.lock().expect("db mutex poisoned");
		let mut tags = crate::settings::get(&db, "booru_tag_blacklist")?
			.split(',')
			.map(|t| t.trim().to_ascii_lowercase())
			.filter(|t| !t.is_empty())
			.collect::<Vec<_>>();
		if let Some(guild) = guild
			&& let Some(config) = db.get_channel_config(guild, channel)?
		{
			tags.extend(config.banned_tags);
		}
		Ok(tags)
	}
	pub(crate) fn delivery_lock(&self, channel: i64) -> Arc<tokio::sync::Mutex<()>> {
		let mut locks = self.delivery_locks.lock().expect("delivery mutex poisoned");
		locks.retain(|_, value| value.strong_count() > 0);
		if let Some(lock) = locks.get(&channel).and_then(Weak::upgrade) {
			return lock;
		}
		let lock = Arc::new(tokio::sync::Mutex::new(()));
		locks.insert(channel, Arc::downgrade(&lock));
		lock
	}
	pub(crate) async fn reconcile_commands(
		&self,
		http: &serenity::http::Http,
		force: bool,
	) -> Result<()> {
		let interval = Duration::from_secs(self.setting_number("command_sync_interval_secs")?);
		if !force
			&& self
				.last_command_sync
				.lock()
				.expect("sync mutex poisoned")
				.elapsed() < interval
		{
			return Ok(());
		}
		let _sync = self.command_sync.lock().await;
		if !force
			&& self
				.last_command_sync
				.lock()
				.expect("sync mutex poisoned")
				.elapsed() < interval
		{
			return Ok(());
		}
		let lang =
			{ crate::settings::get(&self.db.lock().expect("db mutex poisoned"), "app_lang")? };
		register_commands_for_servers(http, Arc::clone(&self.db), &I18n::load(&lang)?).await?;
		*self.last_command_sync.lock().expect("sync mutex poisoned") = Instant::now();
		Ok(())
	}
	pub(crate) fn channel_i18n(&self, guild_id: Option<i64>, channel_id: i64) -> I18n {
		let Some(guild_id) = guild_id else {
			return (*self.i18n).clone();
		};

		let db = self.db.lock().expect("db mutex poisoned");
		if let Ok(Some(cfg)) = db.get_channel_config(guild_id, channel_id)
			&& let Some(ref lang) = cfg.language
			&& let Some(i18n) = self.i18ns.get(&lang.trim().to_ascii_lowercase())
		{
			return i18n.clone();
		}
		crate::settings::get(&db, "app_lang")
			.ok()
			.and_then(|lang| I18n::load(&lang).ok())
			.unwrap_or_else(|| (*self.i18n).clone())
	}

	pub(crate) async fn respond(
		&self,
		http: &serenity::http::Http,
		command: &CommandInteraction,
		content: impl Into<String>,
	) {
		let result = async {
			command.defer_ephemeral(http).await?;
			responses::edit_interaction(http, &self.discord_pacer()?, command, content).await?;
			Ok::<_, anyhow::Error>(())
		}
		.await;
		if let Err(err) = result {
			cli::error(&format!("response failed: {err:#}"), "ERROR");
		}
	}

	pub(crate) async fn edit_deferred(
		&self,
		http: &serenity::http::Http,
		command: &CommandInteraction,
		content: impl Into<String>,
	) {
		let result = async {
			responses::edit_interaction(http, &self.discord_pacer()?, command, content).await?;
			Ok::<_, anyhow::Error>(())
		}
		.await;
		if let Err(err) = result {
			cli::error(&format!("response edit failed: {err:#}"), "ERROR");
		}
	}

	pub(crate) async fn send_image_response(
		&self,
		http: &serenity::http::Http,
		command: &CommandInteraction,
		booru: &BooruConfig,
		image: &ImageResult,
	) -> Result<String> {
		let pacer = self.discord_pacer()?;
		if booru.embed_image {
			match self
				.fetch_client()?
				.inline_image(booru, &image.image_url)
				.await
			{
				Ok(inline_image) => {
					responses::log_final_selection(image, &inline_image);
					responses::edit_interaction_with_inline_image_dm(
						http,
						&pacer,
						&self.cached_posts,
						command,
						booru,
						image,
						inline_image,
					)
					.await
				}
				Err(err) => Err(err),
			}
		} else {
			cli::final_kept(
				image.post_url.as_deref(),
				image.upstream_source_url.as_deref(),
				&image.image_url,
			);
			responses::edit_interaction_with_dm(
				http,
				&pacer,
				&self.cached_posts,
				command,
				booru,
				image,
			)
			.await
		}
	}
}

fn db_setting_string(db: &Database, key: &str, default: &str) -> String {
	db.get_setting(key)
		.ok()
		.flatten()
		.unwrap_or_else(|| default.to_string())
}

fn db_setting_u64(db: &Database, key: &str, default: u64) -> u64 {
	db.get_setting(key)
		.ok()
		.flatten()
		.and_then(|v| v.parse().ok())
		.unwrap_or(default)
}

fn db_setting_usize(db: &Database, key: &str, default: usize) -> usize {
	db.get_setting(key)
		.ok()
		.flatten()
		.and_then(|v| v.parse().ok())
		.unwrap_or(default)
}

fn db_setting_duration_ms(db: &Database, key: &str, default: u64) -> Duration {
	Duration::from_millis(db_setting_u64(db, key, default))
}

fn db_setting_string_list(db: &Database, key: &str, default: &str) -> Vec<String> {
	let raw = db_setting_string(db, key, default);
	if raw.is_empty() {
		return Vec::new();
	}
	raw.split(',')
		.map(|s| s.trim().to_ascii_lowercase())
		.filter(|s| !s.is_empty())
		.collect()
}

async fn connected_guilds(
	http: &serenity::http::Http,
) -> Result<Vec<serenity::model::guild::GuildInfo>> {
	let mut guilds = Vec::new();
	let mut after = None;

	loop {
		let page = http
			.get_guilds(after.map(GuildPagination::After), Some(200))
			.await
			.context("failed to list connected servers")?;
		if page.is_empty() {
			break;
		}

		after = page.last().map(|guild| guild.id);
		let done = page.len() < 200;
		guilds.extend(page);
		if done {
			break;
		}
	}

	Ok(guilds)
}

fn build_commands(
	db: &Database,
	i18n: &I18n,
	server_validated: bool,
	guild_id: Option<i64>,
) -> Result<Vec<CreateCommand>> {
	let mut commands = Vec::new();

	if server_validated {
		let boorus = db.get_enabled_boorus()?;
		for booru in &boorus {
			let description = if booru.description.is_empty() {
				booru.name.clone()
			} else {
				booru.description.clone()
			};
			let cmd = CreateCommand::new(custom_command_name(&booru.name))
				.description(description)
				.add_option(
					CreateCommandOption::new(
						CommandOptionType::String,
						"tag_1",
						i18n.required_tag_option_description(),
					)
					.required(true),
				);

			let tag_count = if booru.max_tags == 0 {
				9
			} else {
				booru.max_tags
			};
			let cmd = (2..=tag_count).fold(cmd, |cmd, index| {
				cmd.add_option(
					CreateCommandOption::new(
						CommandOptionType::String,
						format!("tag_{index}"),
						i18n.custom_tag_option_description(),
					)
					.required(false),
				)
			});

			commands.push(cmd);
		}

		let whitelisted = match guild_id {
			Some(gid) => db.get_server_tags(gid)?,
			None => Vec::new(),
		};
		let whitelisted_set: std::collections::HashSet<String> =
			whitelisted.iter().cloned().collect();

		let pattern_names = db.get_unique_pattern_names()?;
		for name in &pattern_names {
			if !whitelisted_set.contains(name) {
				continue;
			}
			let booru_ids = db.get_booru_ids_for_pattern(name)?;
			let enabled_count = booru_ids.iter().try_fold(0usize, |count, &id| {
				let booru = db.get_booru_by_id(id)?;
				Ok::<_, anyhow::Error>(count + usize::from(booru.is_some_and(|b| b.enabled)))
			})?;

			if enabled_count > 0 {
				commands.push(
					CreateCommand::new(discord_name_component(name))
						.description(i18n.pattern_command_description(name)),
				);
			}
		}

		commands.push(
			CreateCommand::new("art-history")
				.description(i18n.art_history_command_description())
				.add_option(
					CreateCommandOption::new(
						CommandOptionType::Integer,
						"previous_arts",
						i18n.art_history_option_description(),
					)
					.required(true)
					.min_int_value(1),
				),
		);
	}

	commands.push(
		CreateCommand::new("administrate").description(i18n.administrate_command_description()),
	);
	commands.push(
		CreateCommand::new("reload").description("Reload configuration and restart the gateway"),
	);
	commands.push(
		CreateCommand::new("booru-test")
			.description("Test a provider (administrator only)")
			.add_option(
				CreateCommandOption::new(CommandOptionType::String, "booru", "Provider name")
					.required(true),
			)
			.add_option(
				CreateCommandOption::new(
					CommandOptionType::String,
					"tags",
					"Comma-separated test tags",
				)
				.required(true),
			),
	);
	commands.push(
		CreateCommand::new("tag-test")
			.description("Test an enabled server tag")
			.add_option(
				CreateCommandOption::new(CommandOptionType::String, "tag", "Defined tag name")
					.required(true),
			),
	);
	let mut names = std::collections::HashSet::new();
	anyhow::ensure!(
		commands.len() <= 100,
		"a guild cannot have more than 100 slash commands"
	);
	for command in &commands {
		let value = serde_json::to_value(command)?;
		let name = value["name"].as_str().context("command has no name")?;
		crate::config::validate_command_name(name)?;
		anyhow::ensure!(
			names.insert(name.to_string()),
			"duplicate slash command: {name}"
		);
		let description = value["description"].as_str().unwrap_or_default();
		anyhow::ensure!(
			(1..=100).contains(&description.chars().count()),
			"command {name} description must be 1..100 characters"
		);
	}
	Ok(commands)
}

async fn register_commands_for_servers(
	http: &serenity::http::Http,
	db: Arc<Mutex<Database>>,
	i18n: &I18n,
) -> Result<()> {
	let guilds = connected_guilds(http).await?;
	{
		let db = db.lock().expect("db mutex poisoned");
		for guild in &guilds {
			db.add_server(guild.id.get() as i64, &guild.name)?;
		}

		let validated_servers = db.get_validated_servers()?;
		cli::init_status(
			guilds.len(),
			validated_servers.len(),
			db.count_channels()?,
			db.count_server_tag_whitelist()?,
		);
	}

	let mut sets = Vec::new();
	for guild in &guilds {
		let commands = {
			let db = db.lock().expect("db mutex poisoned");
			let guild_id = guild.id.get() as i64;
			let server_validated = db
				.get_server(guild_id)?
				.is_some_and(|server| server.validated);
			build_commands(&db, i18n, server_validated, Some(guild_id))?
		};
		sets.push((guild.id, commands));
	}
	// Validate every replacement before deleting any old global or guild command.
	serenity::all::Command::set_global_commands(http, Vec::new()).await?;
	for (guild, commands) in sets {
		guild.set_commands(http, commands).await?;
	}
	cli::commands_pushed();
	cli::server_ready();
	Ok(())
}

fn load_settings_from_db(
	db: &Database,
	secrets: &SecretConfig,
) -> Result<(String, Duration, usize, usize, Vec<String>, RuntimeValues)> {
	for (key, value) in db.get_all_settings()? {
		crate::settings::validate(&key, &value)?;
	}
	let language = db_setting_string(db, "app_lang", "en");
	let api_rate_pace = db_setting_duration_ms(db, "api_rate_pace_ms", 0);
	let fetch_retry_limit = db_setting_usize(db, "booru_fetch_retry_limit", 3);
	let history_limit = db_setting_usize(db, "booru_source_link_history_limit", 1000);
	let tag_blacklist = db_setting_string_list(db, "booru_tag_blacklist", "");

	let secrets_map = HashMap::from([("discord_token".to_string(), secrets.discord_token.clone())]);
	let settings_map = db.get_all_settings()?.into_iter().collect();
	let mut custom_map: HashMap<String, HashMap<String, String>> = HashMap::new();
	for parameter in db.get_all_booru_custom_parameters()? {
		custom_map
			.entry(parameter.booru_name)
			.or_default()
			.insert(parameter.key, parameter.value);
	}

	let runtime_values = RuntimeValues {
		secrets: secrets_map,
		settings: settings_map,
		custom: custom_map,
	};

	Ok((
		language,
		api_rate_pace,
		fetch_retry_limit,
		history_limit,
		tag_blacklist,
		runtime_values,
	))
}

pub(crate) async fn run() -> Result<()> {
	anyhow::ensure!(
		env::args().len() == 1,
		"no command-line options are supported; configure runtime paths with environment variables"
	);
	let db_path = env::var("RANDOBOORU_DB_PATH").unwrap_or_else(|_| "randobooru.sqlite3".into());

	let mut is_reload = false;
	loop {
		let secrets = secrets::parse()?;
		if !run_bot(&db_path, &secrets, is_reload).await? {
			return Ok(());
		}
		is_reload = true;
	}
}

async fn run_bot(db_path: &str, secrets: &SecretConfig, is_reload: bool) -> Result<bool> {
	if is_reload {
		cli::bot_reloading();
	} else {
		cli::bot_loading();
	}

	let (
		language,
		api_rate_pace,
		_fetch_retry_limit,
		history_limit,
		_tag_blacklist,
		runtime_values,
	) = {
		let db = Database::open(db_path, 1000)?;
		cli::checking_db();
		let cleaned = db.check_and_clean()?;
		if cleaned > 0 {
			cli::db_cleaned(cleaned);
		}
		load_settings_from_db(&db, secrets)?
	};

	let db = Database::open(db_path, history_limit)?;
	let db = Arc::new(Mutex::new(db));
	let discord_pace = {
		crate::settings::number(
			&db.lock().expect("db mutex poisoned"),
			"discord_api_pace_ms",
		)?
	};
	let pacer = ApiPacer::new(Duration::from_millis(discord_pace));
	let i18n = Arc::new(I18n::load(&language)?);
	let mut i18ns = HashMap::new();
	for lang in I18n::available_languages() {
		if let Ok(inst) = I18n::load(lang) {
			i18ns.insert(lang.to_string(), inst);
		}
	}
	let runtime_values = Arc::new(runtime_values);
	let jobs = Arc::new(JobTracker::new());
	let reload_requested = Arc::new(AtomicBool::new(false));
	let reload_notify = Arc::new(Notify::new());
	let cached_posts = Arc::new(PostCache::new(Arc::clone(&db)));

	let handler = Handler {
		db: Arc::clone(&db),
		booru: BooruClient::new(ApiPacer::new(api_rate_pace), Arc::clone(&runtime_values))?,
		pacer,
		i18n: Arc::clone(&i18n),
		i18ns,
		admin_user_id: secrets.admin_user_id,
		jobs: Arc::clone(&jobs),
		reload_requested: Arc::clone(&reload_requested),
		reload_notify: Arc::clone(&reload_notify),
		cached_posts: Arc::clone(&cached_posts),
		admin_state: admin_embed::PanelStore::default(),
		command_sync: tokio::sync::Mutex::new(()),
		last_command_sync: Mutex::new(Instant::now()),
		delivery_locks: Mutex::new(HashMap::new()),
		discord_token: secrets.discord_token.clone(),
	};

	let application_id = ApplicationId::new(secrets.discord_application_id);
	let mut http = serenity::http::Http::new(&secrets.discord_token);
	http.default_allowed_mentions = Some(serenity::all::CreateAllowedMentions::new());
	http.set_application_id(application_id);

	register_commands_for_servers(&http, Arc::clone(&handler.db), &i18n).await?;

	let intents = GatewayIntents::GUILDS;
	let mut client = serenity::client::ClientBuilder::new_with_http(http, intents)
		.application_id(application_id)
		.event_handler(handler)
		.await
		.context("failed to create discord client")?;

	let cache_cleanup = Arc::clone(&cached_posts);
	let cache_cleanup_task = tokio::spawn(async move {
		let mut interval = tokio::time::interval(Duration::from_secs(600));
		loop {
			interval.tick().await;
			if let Err(err) = cache_cleanup.cleanup() {
				cli::error(&format!("{err:#}"), "ERROR");
			}
		}
	});

	let manager = Arc::clone(&client.shard_manager);
	let mut gateway = tokio::spawn(async move { client.start().await });
	let stopped = tokio::select! {
		result = &mut gateway => Some(result),
		_ = reload_notify.notified() => None,
		_ = shutdown_signal() => { jobs.begin_reload(); None },
	};
	cache_cleanup_task.abort();
	if let Some(result) = stopped {
		result
			.context("gateway task panicked")?
			.context("gateway failed")?;
		anyhow::bail!("gateway stopped unexpectedly");
	}
	if tokio::time::timeout(Duration::from_secs(30), jobs.wait_idle())
		.await
		.is_err()
	{
		cli::error("active jobs exceeded shutdown deadline", "ERROR");
		jobs.cancel_all();
		tokio::time::timeout(Duration::from_secs(5), jobs.wait_idle())
			.await
			.context("could not cancel active jobs safely; refusing to start a second client")?;
	}
	// Keep Client::start alive while the manager closes and joins every shard.
	tokio::time::timeout(Duration::from_secs(10), manager.shutdown_all())
		.await
		.context("gateway shutdown timed out")?;
	tokio::time::timeout(Duration::from_secs(5), &mut gateway)
		.await
		.context("gateway did not finish shutdown")?
		.context("gateway task panicked")??;
	cli::unloaded_everything();
	let reload = reload_requested.load(Ordering::Acquire);
	if reload {
		cli::bot_reloaded();
	}
	Ok(reload)
}

async fn shutdown_signal() {
	#[cfg(unix)]
	{
		match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
			Ok(mut term) => {
				tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
			}
			Err(err) => {
				cli::error(&format!("cannot install shutdown signal: {err}"), "ERROR");
				let _ = tokio::signal::ctrl_c().await;
			}
		}
	}
	#[cfg(not(unix))]
	let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
	use crate::config::JsonPathSegment;

	use super::*;

	#[test]
	fn formats_recent_art_links_as_plain_urls() {
		let output = responses::format_recent_art_links(&[
			"https://example.test/1".to_string(),
			"https://example.test/2".to_string(),
		]);

		assert_eq!(
			output,
			"1. <https://example.test/1>\n2. <https://example.test/2>"
		);
	}

	#[test]
	fn builds_custom_command_name() {
		assert_eq!(custom_command_name("test"), "test-custom");
	}

	#[test]
	fn builds_custom_command_name_with_special_chars() {
		assert_eq!(custom_command_name("test.booru!"), "test-booru-custom");
	}

	#[test]
	fn uses_nine_total_custom_tag_options_when_unlimited() {
		let booru = BooruConfig {
			name: "test".to_string(),
			embed_image: false,
			max_tags: 0,
			page_size: 100,
			page_base: 1,
			tag_separator: " ".to_string(),
			encode_tag_separator: true,
			tag_spaces_as_plus: false,
			character_space_replacement: "_".to_string(),
			pace_ms: 0,
			count_url: Some("https://test/count".to_string()),
			count_path: vec![],
			posts_url: "https://test/posts".to_string(),
			posts_path: vec![],
			file_url_path: vec![JsonPathSegment::Key("file_url".to_string())],
			post_url: None,
			source_url_path: vec![],
			detail_url: None,
			detail_id_path: vec![],
			detail_file_url_path: vec![],
			detail_source_url_path: vec![],
			headers: std::collections::HashMap::new(),
			env_params: vec![],
			fetch_options: crate::config::FetchOptions::default(),
		};

		assert_eq!(super::responses::custom_tag_count(&booru), 9);
	}
}
