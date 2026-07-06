use std::{
	collections::HashMap,
	env,
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc, Mutex,
	},
	time::Duration,
};

use anyhow::{Context as _, Result};
use serenity::{
	all::{
		ApplicationId, CommandInteraction, CommandOptionType, Context, CreateCommand,
		CreateCommandOption, EventHandler, GatewayIntents, Interaction, Ready, ResolvedValue,
	},
	async_trait,
	http::GuildPagination,
	Client,
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
mod commands;
mod responses;
mod secrets;

pub(crate) use admin::execute_admin_action;
pub(crate) use commands::JobTracker;
pub(crate) use responses::{custom_command_name, discord_name_component};
pub(crate) use secrets::SecretConfig;

pub struct Handler {
	db: Arc<Mutex<Database>>,
	booru: BooruClient,
	pacer: ApiPacer,
	i18n: Arc<I18n>,
	i18ns: HashMap<String, I18n>,
	admin_user_id: u64,
	booru_fetch_retry_limit: usize,
	booru_tag_blacklist: Arc<[String]>,
	jobs: Arc<JobTracker>,
	reload_requested: Arc<AtomicBool>,
	reload_notify: Arc<Notify>,
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
	async fn ready(&self, _ctx: Context, _ready: Ready) {}

	async fn guild_create(
		&self,
		_ctx: Context,
		guild: serenity::all::Guild,
		_is_new: Option<bool>,
	) {
		let guild_id = guild.id.get() as i64;
		let name = guild.name.clone();
		let db = self.db.lock().expect("db mutex poisoned");
		let _ = db.add_server(guild_id, &name);
	}

	async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
		commands::handle_interaction(self, &ctx, interaction).await;
	}
}

impl Handler {
	pub(crate) fn channel_i18n(&self, guild_id: Option<i64>, channel_id: i64) -> I18n {
		let Some(guild_id) = guild_id else {
			return (*self.i18n).clone();
		};

		let db = self.db.lock().expect("db mutex poisoned");
		if let Ok(Some(cfg)) = db.get_channel_config(guild_id, channel_id) {
			if let Some(ref lang) = cfg.language {
				if let Some(i18n) = self.i18ns.get(lang) {
					return i18n.clone();
				}
			}
		}
		(*self.i18n).clone()
	}

	pub(crate) async fn respond(
		&self,
		http: &serenity::http::Http,
		command: &CommandInteraction,
		content: impl Into<String>,
	) {
		if command.defer(http).await.is_ok() {
			let _ = responses::edit_interaction(http, &self.pacer, command, content).await;
		}
	}

	pub(crate) async fn edit_deferred(
		&self,
		http: &serenity::http::Http,
		command: &CommandInteraction,
		content: impl Into<String>,
	) {
		let _ = responses::edit_interaction(http, &self.pacer, command, content).await;
	}

	pub(crate) async fn send_image_response(
		&self,
		http: &serenity::http::Http,
		command: &CommandInteraction,
		booru: &BooruConfig,
		image: &ImageResult,
	) {
		if booru.embed_image {
			match self.booru.inline_image(booru, &image.image_url).await {
				Ok(inline_image) => {
					responses::log_final_selection(image, &inline_image);
					let _ = responses::edit_interaction_with_inline_image(
						http,
						&self.pacer,
						booru,
						command,
						image,
						inline_image,
					)
					.await;
				}
				Err(err) => {
					let _ = responses::edit_interaction(
						http,
						&self.pacer,
						command,
						self.i18n.could_not_find_image(&err.to_string()),
					)
					.await;
				}
			}
		} else {
			cli::final_kept(
				image.post_url.as_deref(),
				image.upstream_source_url.as_deref(),
				&image.image_url,
			);
			let _ = responses::edit_interaction(
				http,
				&self.pacer,
				command,
				responses::format_image_response(booru, image, false),
			)
			.await;
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
) -> Result<Vec<CreateCommand>> {
	let mut commands = Vec::new();

	if server_validated {
		let boorus = db.get_enabled_boorus()?;
		for booru in &boorus {
			let cmd = CreateCommand::new(custom_command_name(&booru.name))
				.description(i18n.custom_command_description(&booru.name))
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

		let pattern_names = db.get_unique_pattern_names()?;
		for name in &pattern_names {
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
		commands
			.push(CreateCommand::new("reload").description(i18n.reload_toml_command_description()));
	}

	commands.push(
		CreateCommand::new("administrate")
			.description(i18n.administrate_command_description())
			.add_option(
				CreateCommandOption::new(
					CommandOptionType::String,
					"action",
					i18n.administrate_action_description(),
				)
				.required(true),
			),
	);
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
			db.count_enabled_pattern_commands()?,
		);
	}

	serenity::all::Command::set_global_commands(http, Vec::new()).await?;
	for guild in &guilds {
		guild.id.set_commands(http, Vec::new()).await?;
	}
	cli::commands_cleaned();

	for guild in &guilds {
		let commands = {
			let db = db.lock().expect("db mutex poisoned");
			let server_validated = db
				.get_server(guild.id.get() as i64)?
				.is_some_and(|server| server.validated);
			build_commands(&db, i18n, server_validated)?
		};
		guild.id.set_commands(http, commands).await?;
	}
	cli::commands_pushed();
	cli::server_ready();
	Ok(())
}

fn load_settings_from_db(
	db: &Database,
	secrets: &SecretConfig,
) -> Result<(String, Duration, usize, usize, Vec<String>, RuntimeValues)> {
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
	let doctor = env::args().nth(1).as_deref() == Some("doctor");

	let secrets = secrets::parse()?;
	let db_path = "randobooru.sqlite3";

	loop {
		if !run_bot(doctor, db_path, &secrets).await? || doctor {
			return Ok(());
		}
	}
}

async fn run_bot(_doctor: bool, db_path: &str, secrets: &SecretConfig) -> Result<bool> {
	let (language, api_rate_pace, fetch_retry_limit, history_limit, tag_blacklist, runtime_values) = {
		let db = Database::open(db_path, 1000)?;
		load_settings_from_db(&db, secrets)?
	};

	let db = Database::open(db_path, history_limit)?;
	let db = Arc::new(Mutex::new(db));
	let pacer = ApiPacer::new(api_rate_pace);
	let i18n = Arc::new(I18n::load(&language)?);
	let mut i18ns = HashMap::new();
	for lang in I18n::available_languages() {
		if let Ok(inst) = I18n::load(lang) {
			i18ns.insert(lang.to_string(), inst);
		}
	}
	let runtime_values = Arc::new(runtime_values);
	let booru_tag_blacklist: Arc<[String]> = tag_blacklist.into();
	let jobs = Arc::new(JobTracker::new());
	let reload_requested = Arc::new(AtomicBool::new(false));
	let reload_notify = Arc::new(Notify::new());

	let handler = Handler {
		db: Arc::clone(&db),
		booru: BooruClient::new(pacer.clone(), Arc::clone(&runtime_values)),
		pacer,
		i18n: Arc::clone(&i18n),
		i18ns,
		admin_user_id: secrets.admin_user_id,
		booru_fetch_retry_limit: fetch_retry_limit,
		booru_tag_blacklist: Arc::clone(&booru_tag_blacklist),
		jobs,
		reload_requested: Arc::clone(&reload_requested),
		reload_notify: Arc::clone(&reload_notify),
	};

	let application_id = ApplicationId::new(secrets.discord_application_id);
	let http = serenity::http::Http::new(&secrets.discord_token);
	http.set_application_id(application_id);

	cli::bot_reloaded();
	register_commands_for_servers(&http, Arc::clone(&handler.db), &i18n).await?;

	let intents = GatewayIntents::empty();
	let mut client = Client::builder(&secrets.discord_token, intents)
		.application_id(application_id)
		.event_handler(handler)
		.await
		.context("failed to create discord client")?;

	tokio::select! {
		result = client.start() => {
			result.context("discord client stopped")?;
		}
		_ = reload_notify.notified() => {}
	}

	Ok(reload_requested.load(Ordering::Acquire))
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
			count_url: "https://test/count".to_string(),
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
		};

		assert_eq!(super::responses::custom_tag_count(&booru), 9);
	}
}
