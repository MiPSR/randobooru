use super::{Handler, post_cache::unique_id};
use crate::{
	config::BooruConfig,
	db::{ChannelConfig, Database},
};
use anyhow::{Context as _, Result, ensure};
use rand::RngExt;
use serenity::all::{
	ActionRow, ActionRowComponent, ButtonStyle, ChannelId, ComponentInteraction, Context,
	CreateActionRow, CreateAttachment, CreateButton, CreateEmbed, CreateInputText,
	CreateInteractionResponse, CreateInteractionResponseMessage, CreateModal,
	EditInteractionResponse, InputTextStyle, ModalInteraction,
};
use std::{
	collections::HashMap,
	sync::Mutex,
	time::{Duration, Instant},
};

const PAGE_SIZE: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
	Page,
	Select,
	Form,
	Confirm,
	Apply,
}

#[derive(Clone)]
struct Action {
	category: String,
	operation: String,
	target: Option<String>,
	page: usize,
	stage: Stage,
	values: HashMap<String, String>,
}
impl Action {
	fn new(category: &str, operation: &str, stage: Stage) -> Self {
		Self {
			category: category.into(),
			operation: operation.into(),
			target: None,
			page: 0,
			stage,
			values: HashMap::new(),
		}
	}
	fn target(mut self, value: impl Into<String>) -> Self {
		self.target = Some(value.into());
		self
	}
	fn page(mut self, page: usize) -> Self {
		self.page = page;
		self
	}
}

struct Entry {
	user: u64,
	guild: Option<i64>,
	expires: Instant,
	action: Action,
}
#[derive(Default)]
pub(crate) struct PanelStore {
	entries: Mutex<HashMap<String, Entry>>,
}
impl PanelStore {
	fn put(&self, user: u64, guild: Option<i64>, action: Action) -> String {
		let mut entries = self.entries.lock().expect("panel mutex poisoned");
		entries.retain(|_, v| v.expires > Instant::now());
		// Bound abandoned panels without unbounded allocation under repeated opens.
		if entries.len() >= 10_000 {
			entries.clear();
		}
		let id = format!("admin:{}", unique_id());
		entries.insert(
			id.clone(),
			Entry {
				user,
				guild,
				expires: Instant::now() + Duration::from_secs(3600),
				action,
			},
		);
		id
	}
	fn get(&self, id: &str, user: u64, guild: Option<i64>, consume: bool) -> Result<Action> {
		let mut entries = self.entries.lock().expect("panel mutex poisoned");
		let entry = entries
			.get(id)
			.context("This panel expired. Open /administrate again.")?;
		ensure!(
			entry.user == user && entry.guild == guild && entry.expires > Instant::now(),
			"This panel belongs to another user or has expired."
		);
		let action = entry.action.clone();
		if consume {
			entries.remove(id);
		}
		Ok(action)
	}
}

fn check_scope(
	owner: bool,
	moderator: bool,
	guild: Option<i64>,
	category: &str,
	target_guild: Option<i64>,
) -> Result<()> {
	ensure!(
		owner || moderator,
		"Administration requires administrator or guild-moderator access."
	);
	if owner {
		return Ok(());
	}
	ensure!(
		guild.is_some() && matches!(category, "main" | "channels" | "server_tags" | "reload"),
		"Only the administrator can manage shared configuration."
	);
	ensure!(
		target_guild.is_none() || target_guild == guild,
		"Moderators can only manage their own guild."
	);
	Ok(())
}

fn authorize(handler: &Handler, user: u64, guild: Option<i64>, action: &Action) -> Result<()> {
	let owner = user == handler.admin_user_id;
	let moderator = handler
		.db
		.lock()
		.expect("db mutex poisoned")
		.is_moderator(user as i64, guild)?;
	let target_guild = if action.category == "channels" {
		action
			.target
			.as_deref()
			.and_then(|v| v.split('/').next())
			.and_then(|v| v.parse().ok())
			.or_else(|| action.values.get("guild").and_then(|v| v.parse().ok()))
	} else {
		None
	};
	check_scope(owner, moderator, guild, &action.category, target_guild)
}

pub(crate) async fn handle_administrate_embed(
	handler: &Handler,
	command: &serenity::all::CommandInteraction,
	ctx: &Context,
	_user: i64,
) -> Result<()> {
	let guild = command.guild_id.map(|g| g.get() as i64);
	let user = command.user.id.get();
	let action = Action::new("main", "", Stage::Page);
	if authorize(handler, user, guild, &action).is_err() {
		command
			.create_response(
				&ctx.http,
				CreateInteractionResponse::Message(
					CreateInteractionResponseMessage::new()
						.content(handler.i18n.admin_only())
						.ephemeral(true),
				),
			)
			.await?;
		return Ok(());
	}
	command.defer_ephemeral(&ctx.http).await?;
	let (embed, rows) = page(handler, user, guild, &action)?;
	command
		.edit_response(
			&ctx.http,
			EditInteractionResponse::new().embed(embed).components(rows),
		)
		.await?;
	Ok(())
}

pub(crate) async fn handle_component_interaction(
	handler: &Handler,
	v: &ComponentInteraction,
	ctx: &Context,
) -> Result<()> {
	let user = v.user.id.get();
	let guild = v.guild_id.map(|g| g.get() as i64);
	let action = match handler
		.admin_state
		.get(&v.data.custom_id, user, guild, false)
		.and_then(|a| {
			authorize(handler, user, guild, &a)?;
			Ok(a)
		}) {
		Ok(a) => a,
		Err(err) => {
			v.create_response(
				&ctx.http,
				CreateInteractionResponse::Message(
					CreateInteractionResponseMessage::new()
						.content(err.to_string())
						.ephemeral(true),
				),
			)
			.await?;
			return Ok(());
		}
	};
	match action.stage {
		Stage::Form => {
			let fields = form_fields(handler, guild, &action)?;
			let id = handler.admin_state.put(user, guild, action.clone());
			let rows = fields
				.into_iter()
				.map(|(id, label, value, required, paragraph)| {
					CreateActionRow::InputText(
						CreateInputText::new(
							if paragraph {
								InputTextStyle::Paragraph
							} else {
								InputTextStyle::Short
							},
							label,
							id,
						)
						.required(required)
						.value(value)
						.max_length(4000),
					)
				})
				.collect();
			v.create_response(
				&ctx.http,
				CreateInteractionResponse::Modal(
					CreateModal::new(
						id,
						truncate(&format!("{}: {}", action.category, action.operation), 45),
					)
					.components(rows),
				),
			)
			.await?;
		}
		Stage::Page | Stage::Select => {
			let (embed, rows) = page(handler, user, guild, &action)?;
			v.create_response(
				&ctx.http,
				CreateInteractionResponse::UpdateMessage(
					CreateInteractionResponseMessage::new()
						.content("")
						.embed(embed)
						.components(rows),
				),
			)
			.await?;
		}
		Stage::Confirm => {
			let (embed, rows) = confirmation(handler, user, guild, &action);
			v.create_response(
				&ctx.http,
				CreateInteractionResponse::UpdateMessage(
					CreateInteractionResponseMessage::new()
						.content("")
						.embed(embed)
						.components(rows),
				),
			)
			.await?;
		}
		Stage::Apply => {
			v.create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
				.await?;
			handler
				.admin_state
				.get(&v.data.custom_id, user, guild, true)?;
			if action.operation == "test" {
				let (booru, tags, blacklist) =
					test_candidate(handler, guild, v.channel_id.get() as i64, &action)?;
				let image = handler
					.fetch_client()?
					.random_image(&booru, &tags, &blacklist, None)
					.await?;
				let mut edit = EditInteractionResponse::new()
					.content(super::responses::format_image_response(
						&booru, &image, false,
					))
					.embeds(Vec::new())
					.components(Vec::new());
				if booru.embed_image {
					let inline = handler
						.fetch_client()?
						.inline_image(&booru, &image.image_url)
						.await?;
					edit =
						edit.new_attachment(CreateAttachment::bytes(inline.data, inline.filename));
				}
				v.edit_response(&ctx.http, edit).await?;
				return Ok(());
			}
			let message = if action.category == "reload" {
				super::commands::request_reload(handler)?;
				"Reload requested. Waiting for active jobs before closing the old gateway.".into()
			} else {
				apply(handler, user, guild, &action)?
			};
			let (embed, rows) = page(
				handler,
				user,
				guild,
				&Action::new(&action.category, "", Stage::Page),
			)?;
			v.edit_response(
				&ctx.http,
				EditInteractionResponse::new()
					.content(message)
					.embed(embed)
					.components(rows),
			)
			.await?;
			if action.category != "reload" {
				handler.reconcile_commands(&ctx.http, true).await?;
			}
		}
	}
	Ok(())
}

pub(crate) async fn handle_modal_interaction(
	handler: &Handler,
	v: &ModalInteraction,
	ctx: &Context,
) -> Result<()> {
	let user = v.user.id.get();
	let guild = v.guild_id.map(|g| g.get() as i64);
	let mut action = match handler
		.admin_state
		.get(&v.data.custom_id, user, guild, true)
		.and_then(|a| {
			authorize(handler, user, guild, &a)?;
			Ok(a)
		}) {
		Ok(a) => a,
		Err(err) => {
			v.create_response(
				&ctx.http,
				CreateInteractionResponse::Message(
					CreateInteractionResponseMessage::new()
						.content(err.to_string())
						.ephemeral(true),
				),
			)
			.await?;
			return Ok(());
		}
	};
	v.create_response(
		&ctx.http,
		CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new().ephemeral(true)),
	)
	.await?;
	action.values = values(&v.data.components);
	if let Err(err) = authorize(handler, user, guild, &action) {
		v.edit_response(
			&ctx.http,
			EditInteractionResponse::new().content(err.to_string()),
		)
		.await?;
		return Ok(());
	}
	if action.category == "channels" && action.operation == "add" {
		let gid = field(&action, "guild")
			.parse::<u64>()
			.context("invalid guild ID")?;
		let cid = field(&action, "channel")
			.parse::<u64>()
			.context("invalid channel ID")?;
		let channel = ChannelId::new(cid)
			.to_channel(&ctx.http)
			.await?
			.guild()
			.context("not a server channel")?;
		ensure!(
			channel.guild_id.get() == gid,
			"channel does not belong to the selected guild"
		);
	}
	if action.operation == "test" {
		let (booru, tags, blacklist) =
			test_candidate(handler, guild, v.channel_id.get() as i64, &action)?;
		let image = handler
			.fetch_client()?
			.random_image(&booru, &tags, &blacklist, None)
			.await?;
		let mut response = EditInteractionResponse::new().content(
			super::responses::format_image_response(&booru, &image, false),
		);
		if booru.embed_image {
			let inline = handler
				.fetch_client()?
				.inline_image(&booru, &image.image_url)
				.await?;
			response =
				response.new_attachment(CreateAttachment::bytes(inline.data, inline.filename));
		}
		v.edit_response(&ctx.http, response).await?;
		if let Some(id) = &action.target {
			handler
				.db
				.lock()
				.expect("db mutex poisoned")
				.set_setting(&format!("test_tags:{id}"), field(&action, "tags"))?;
		}
		return Ok(());
	}
	// Validate the proposed mutation before confirmation; no writes until the user confirms.
	validate_form(handler, guild, &action)?;
	action.stage = Stage::Confirm;
	let (embed, rows) = confirmation(handler, user, guild, &action);
	v.edit_response(
		&ctx.http,
		EditInteractionResponse::new().embed(embed).components(rows),
	)
	.await?;
	Ok(())
}

fn values(rows: &[ActionRow]) -> HashMap<String, String> {
	rows.iter()
		.flat_map(|r| r.components.iter())
		.filter_map(|v| match v {
			ActionRowComponent::InputText(v) => {
				Some((v.custom_id.clone(), v.value.clone().unwrap_or_default()))
			}
			_ => None,
		})
		.collect()
}
fn field<'a>(a: &'a Action, key: &str) -> &'a str {
	a.values.get(key).map(String::as_str).unwrap_or("")
}
fn target(a: &Action) -> Result<&str> {
	a.target.as_deref().context("no item selected")
}
fn channel_ids(a: &Action) -> Result<(i64, i64)> {
	let (g, c) = target(a)?
		.split_once('/')
		.context("invalid channel target")?;
	Ok((g.parse()?, c.parse()?))
}
fn button(
	handler: &Handler,
	user: u64,
	guild: Option<i64>,
	label: &str,
	action: Action,
	style: ButtonStyle,
) -> CreateButton {
	CreateButton::new(handler.admin_state.put(user, guild, action))
		.label(truncate(label, 70))
		.style(style)
}

fn confirmation(
	handler: &Handler,
	user: u64,
	guild: Option<i64>,
	action: &Action,
) -> (CreateEmbed, Vec<CreateActionRow>) {
	let mut apply = action.clone();
	apply.stage = Stage::Apply;
	let embed = CreateEmbed::new()
		.title("Confirm configuration change")
		.description(format!(
			"{} / {}{}\nValues containing credentials are intentionally not displayed.",
			action.category,
			action.operation,
			action
				.target
				.as_ref()
				.map(|t| format!(" / {}", truncate(t, 150)))
				.unwrap_or_default()
		))
		.color(0xed4245);
	(
		embed,
		vec![CreateActionRow::Buttons(vec![
			button(handler, user, guild, "Confirm", apply, ButtonStyle::Danger),
			button(
				handler,
				user,
				guild,
				"Cancel",
				Action::new(&action.category, "", Stage::Page),
				ButtonStyle::Secondary,
			),
		])],
	)
}

fn page(
	handler: &Handler,
	user: u64,
	guild: Option<i64>,
	action: &Action,
) -> Result<(CreateEmbed, Vec<CreateActionRow>)> {
	let owner = user == handler.admin_user_id;
	let mut buttons = Vec::new();
	let mut nav = Vec::new();
	let mut description = String::new();
	if action.category == "main" || action.category == "reload" {
		for (category, label) in [
			("boorus", "Boorus"),
			("patterns", "Tag patterns"),
			("server_tags", "Server tags"),
			("channels", "Channels"),
			("settings", "Settings"),
			("servers", "Servers"),
			("moderators", "Moderators"),
		] {
			if owner || matches!(category, "server_tags" | "channels") {
				buttons.push(button(
					handler,
					user,
					guild,
					label,
					Action::new(category, "", Stage::Page),
					ButtonStyle::Primary,
				));
			}
		}
		buttons.push(button(
			handler,
			user,
			guild,
			"Reload",
			Action::new("reload", "reload", Stage::Confirm),
			ButtonStyle::Danger,
		));
		description.push_str("Private administration panel. All actions are re-authorized and bound to your user and server. Changes require confirmation; successful changes apply immediately.\nUse /booru-test, /tag-test and /reload for shortcuts.");
	} else {
		let items = {
			let db = handler.db.lock().expect("db mutex poisoned");
			items(&db, guild, owner, &action.category)?
		};
		let pages = items.len().div_ceil(PAGE_SIZE).max(1);
		let page = action.page.min(pages - 1);
		for (id, label) in items.iter().skip(page * PAGE_SIZE).take(PAGE_SIZE) {
			description.push_str(&format!("{}\n", truncate(label, 350)));
			if action.stage == Stage::Select {
				let stage = if action.operation == "show" {
					Stage::Page
				} else if matches!(
					action.operation.as_str(),
					"delete" | "toggle" | "reset" | "validate" | "unvalidate"
				) {
					Stage::Confirm
				} else if action.category == "patterns" && action.operation == "test" {
					Stage::Apply
				} else {
					Stage::Form
				};
				buttons.push(button(
					handler,
					user,
					guild,
					label,
					Action::new(&action.category, &action.operation, stage).target(id),
					ButtonStyle::Primary,
				));
			}
		}
		if items.is_empty() {
			description.push_str("No items configured.\n");
		}
		description.push_str(&format!("\nPage {}/{}", page + 1, pages));
		if action.stage == Stage::Page {
			let ops: &[(&str, &str)] = match action.category.as_str() {
				"boorus" => &[
					("show", "Inspect"),
					("add", "Add"),
					("edit", "Edit"),
					("delete", "Delete"),
					("toggle", "Toggle"),
					("parameters", "Parameters"),
					("test", "Test"),
				],
				"patterns" => &[
					("add", "Add / replace"),
					("delete", "Delete"),
					("test", "Test"),
				],
				"channels" => &[
					("add", "Add"),
					("edit", "Edit config"),
					("delete", "Remove"),
					("patterns", "Tag blacklist"),
				],
				"settings" => &[
					("add", "Add"),
					("edit", "Edit"),
					("delete", "Delete"),
					("reset", "Reset"),
				],
				"servers" => &[("validate", "Validate"), ("unvalidate", "Unvalidate")],
				"moderators" => &[("add", "Add"), ("delete", "Remove")],
				"server_tags" => &[("add", "Enable tag"), ("delete", "Disable tag")],
				_ => &[],
			};
			for (op, label) in ops {
				buttons.push(button(
					handler,
					user,
					guild,
					label,
					Action::new(
						&action.category,
						op,
						if *op == "add" {
							Stage::Form
						} else {
							Stage::Select
						},
					),
					ButtonStyle::Primary,
				));
			}
		}
		if page > 0 {
			nav.push(button(
				handler,
				user,
				guild,
				"Previous",
				action.clone().page(page - 1),
				ButtonStyle::Secondary,
			));
		}
		if page + 1 < pages {
			nav.push(button(
				handler,
				user,
				guild,
				"Next",
				action.clone().page(page + 1),
				ButtonStyle::Secondary,
			));
		}
		let back = if action.stage == Stage::Select {
			&action.category
		} else {
			"main"
		};
		nav.push(button(
			handler,
			user,
			guild,
			"Back",
			Action::new(back, "", Stage::Page),
			ButtonStyle::Secondary,
		));
	}
	if action.category == "boorus" && action.operation == "show" && action.target.is_some() {
		let row = { provider(&handler.db.lock().expect("db mutex poisoned"), action)? };
		let mut config = BooruConfig::from_row(&row)?;
		for value in config.headers.values_mut() {
			*value = "[hidden]".into();
		}
		let keys = handler
			.db
			.lock()
			.expect("db mutex poisoned")
			.get_booru_custom_parameters(&row.name)?
			.into_iter()
			.map(|p| p.key)
			.collect::<Vec<_>>();
		let mut value = serde_json::to_value(&config)?;
		value["custom_parameter_keys"] = serde_json::json!(keys);
		let text = serde_json::to_string_pretty(&value)?;
		let chunks = text.chars().collect::<Vec<_>>();
		let pages = chunks.len().div_ceil(3400).max(1);
		let p = action.page.min(pages - 1);
		description = format!(
			"{} / page {}/{}\n```json\n{}\n```",
			row.name,
			p + 1,
			pages,
			chunks.iter().skip(p * 3400).take(3400).collect::<String>()
		);
		buttons.clear();
		nav.clear();
		if p > 0 {
			nav.push(button(
				handler,
				user,
				guild,
				"Previous",
				action.clone().page(p - 1),
				ButtonStyle::Secondary,
			));
		}
		if p + 1 < pages {
			nav.push(button(
				handler,
				user,
				guild,
				"Next",
				action.clone().page(p + 1),
				ButtonStyle::Secondary,
			));
		}
		nav.push(button(
			handler,
			user,
			guild,
			"Back",
			Action::new("boorus", "", Stage::Page),
			ButtonStyle::Secondary,
		));
	}
	let mut rows = buttons
		.chunks(4)
		.map(|v| CreateActionRow::Buttons(v.to_vec()))
		.collect::<Vec<_>>();
	if !nav.is_empty() {
		rows.push(CreateActionRow::Buttons(nav));
	}
	ensure!(rows.len() <= 5, "too many panel rows");
	Ok((
		CreateEmbed::new()
			.title(format!("Administration: {}", action.category))
			.description(truncate(&description, 3900))
			.color(0x5865f2),
		rows,
	))
}

fn items(
	db: &Database,
	guild: Option<i64>,
	owner: bool,
	category: &str,
) -> Result<Vec<(String, String)>> {
	Ok(match category {
		"boorus" => db
			.get_all_boorus()?
			.into_iter()
			.map(|b| {
				(
					b.id.to_string(),
					format!(
						"{} [{}] page={} tags={} pace={}ms",
						b.name,
						if b.enabled { "enabled" } else { "disabled" },
						b.page_size,
						b.max_tags,
						b.pace_ms
					),
				)
			})
			.collect(),
		"patterns" => db
			.get_tag_patterns(None)?
			.into_iter()
			.map(|p| {
				let b = db
					.get_booru_by_id(p.booru_id)?
					.context("provider missing")?;
				let tags = db
					.get_pattern_entries(p.id)?
					.into_iter()
					.map(|e| format!("{}{}", if e.is_excluded { "−" } else { "+" }, e.tag))
					.collect::<Vec<_>>()
					.join(", ");
				Ok((
					p.id.to_string(),
					format!("{} / {}: {}", p.name, b.name, tags),
				))
			})
			.collect::<Result<_>>()?,
		"server_tags" => db
			.get_server_tags(guild.context("open this category in a server")?)?
			.into_iter()
			.map(|t| (t.clone(), t))
			.collect(),
		"channels" => {
			let mut out = Vec::new();
			for s in db.get_all_servers()? {
				if !owner && Some(s.guild_id) != guild {
					continue;
				}
				for c in db.get_guild_channels(s.guild_id)? {
					let patterns = db.get_channel_patterns(c.guild_id, c.channel_id)?;
					out.push((
						format!("{}/{}", c.guild_id, c.channel_id),
						format!(
							"{} / #{}: lang={} banned={} blocked={}",
							s.name,
							c.channel_id,
							c.language.as_deref().unwrap_or("default"),
							c.banned_tags.join(","),
							patterns.join(",")
						),
					));
				}
			}
			out
		}
		"settings" => {
			let mut all: HashMap<String, String> = crate::settings::DEFAULTS
				.iter()
				.map(|(k, v)| (k.to_string(), v.to_string()))
				.collect();
			all.extend(db.get_all_settings()?);
			let mut all = all
				.into_iter()
				.map(|(k, v)| {
					let shown = if crate::settings::DEFAULTS.iter().any(|(key, _)| *key == k) {
						v
					} else {
						"[custom value hidden]".into()
					};
					(k.clone(), format!("{k} = {shown}"))
				})
				.collect::<Vec<_>>();
			all.sort();
			all
		}
		"servers" => db
			.get_all_servers()?
			.into_iter()
			.map(|s| {
				(
					s.guild_id.to_string(),
					format!(
						"{} / {} [{}]",
						s.name,
						s.guild_id,
						if s.validated { "validated" } else { "pending" }
					),
				)
			})
			.collect(),
		"moderators" => db
			.list_moderators()?
			.into_iter()
			.filter_map(|(u, g)| g.map(|g| (format!("{g}/{u}"), format!("User {u} / guild {g}"))))
			.collect(),
		_ => Vec::new(),
	})
}

// Modal fields: key, label, prefilled value, required, multiline.
type Field = (&'static str, &'static str, String, bool, bool);
fn form_fields(handler: &Handler, guild: Option<i64>, a: &Action) -> Result<Vec<Field>> {
	let db = handler.db.lock().expect("db mutex poisoned");
	let blank = String::new();
	Ok(match (a.category.as_str(),a.operation.as_str()) {
		("boorus","add")=>vec![("name","Provider name",blank.clone(),true,false),("description","Slash command description",blank.clone(),false,false),("urls","URLs JSON",r#"{"posts_url":"https://example.org/posts?tags={tags}&page={page}&limit={limit}"}"#.into(),true,true),("paths","JSON paths",r#"{"posts_path":[],"file_url_path":["file_url"],"detail_id_path":["id"],"count_path":[],"source_url_path":[],"detail_file_url_path":[],"detail_source_url_path":[]}"#.into(),true,true),("options","Options JSON",r#"{"embed_image":false,"max_tags":9,"page_size":100,"page_base":1,"tag_separator":" ","encode_tag_separator":true,"tag_spaces_as_plus":false,"character_space_replacement":"_","headers":{},"env_params":[],"fetch_options":{}}"#.into(),true,true)],
		("boorus","edit")=>vec![("field","Field name (e.g. fetch_options)",blank.clone(),true,false),("value","New value (null clears optional URLs)",blank.clone(),false,true)],
		("boorus","parameters")=>vec![("operation","set or delete", "set".into(),true,false),("key","Parameter key",blank.clone(),true,false),("value","Value (hidden in panel)",blank.clone(),false,true)],
		("boorus","test")=>vec![("tags","Test tags (comma-separated)",db.get_setting(&format!("test_tags:{}",target(a)?))?.unwrap_or_else(||"solo".into()),true,false)],
		("patterns","add")=>vec![("name","Pattern command name",blank.clone(),true,false),("booru","Provider name",blank.clone(),true,false),("included","Included tags (comma-separated)",blank.clone(),true,true),("excluded","Excluded tags (comma-separated)",blank.clone(),false,true)],
		("server_tags","add")=>vec![("name","Configured pattern name",blank.clone(),true,false)],
		("channels","add")=>vec![("guild","Guild ID",guild.map(|v|v.to_string()).unwrap_or_default(),true,false),("channel","Channel ID",blank.clone(),true,false)],
		("channels","edit")=>{let (g,c)=channel_ids(a)?;let cfg=db.get_channel_config(g,c)?.context("channel not configured")?;vec![("language","Language (blank = default)",cfg.language.unwrap_or_default(),false,false),("banned","Banned tags (comma-separated)",cfg.banned_tags.join(", "),false,true)]},
		("channels","patterns")=>vec![("operation","add or remove", "add".into(),true,false),("name","Whitelisted pattern name",blank.clone(),true,false)],
		("settings","add"|"edit")=>{let key=a.target.clone().unwrap_or_default();let value=if key.is_empty(){blank.clone()}else{crate::settings::get(&db,&key)?};vec![("key","Setting key",key,true,false),("value","Setting value (blank is allowed)",value,false,true)]},
		("moderators","add")=>vec![("guild","Guild ID",guild.map(|v|v.to_string()).unwrap_or_default(),true,false),("user","User ID",blank.clone(),true,false)],
		_=>anyhow::bail!("unsupported form"),
	})
}

fn provider(db: &Database, a: &Action) -> Result<crate::db::BooruRow> {
	db.get_booru_by_id(target(a)?.parse()?)?
		.context("provider no longer exists")
}
fn csv(value: &str) -> Vec<String> {
	value
		.split(',')
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.collect()
}
fn new_provider(a: &Action) -> Result<crate::db::BooruRow> {
	let mut map = serde_json::Map::new();
	for key in ["urls", "paths", "options"] {
		let value: serde_json::Value =
			serde_json::from_str(field(a, key)).with_context(|| format!("invalid {key} JSON"))?;
		let object = value
			.as_object()
			.context("provider inputs must be JSON objects")?;
		for (key, value) in object {
			ensure!(!map.contains_key(key), "duplicate provider field {key}");
			map.insert(key.clone(), value.clone());
		}
	}
	map.insert("description".into(), field(a, "description").into());
	super::admin::booru::from_json(field(a, "name"), &serde_json::to_string(&map)?)
}

fn validate_form(handler: &Handler, guild: Option<i64>, a: &Action) -> Result<()> {
	let db = handler.db.lock().expect("db mutex poisoned");
	match (a.category.as_str(), a.operation.as_str()) {
		("boorus", "add") => {
			new_provider(a)?;
		}
		("boorus", "edit") => {
			let mut row = provider(&db, a)?;
			super::admin::booru::edit_field(&mut row, field(a, "field"), field(a, "value"))?;
		}
		("boorus", "parameters") => {
			ensure!(
				matches!(field(a, "operation"), "set" | "delete"),
				"operation must be set or delete"
			);
			ensure!(
				!field(a, "key").trim().is_empty(),
				"parameter key is required"
			);
		}
		("patterns", "add") => {
			ensure!(
				!field(a, "name").trim().is_empty() && !csv(field(a, "included")).is_empty(),
				"pattern name and included tags are required"
			);
			db.get_booru_by_name(field(a, "booru"))?
				.context("provider not found")?;
		}
		("server_tags", "add") => {
			guild.context("guild context required")?;
			ensure!(
				db.get_unique_pattern_names()?
					.contains(&field(a, "name").to_string()),
				"unknown pattern"
			);
		}
		("channels", "add") => {
			let g = field(a, "guild").parse::<i64>()?;
			let c = field(a, "channel").parse::<i64>()?;
			ensure!(g > 0 && c > 0, "IDs must be positive");
			ensure!(
				!db.has_channel(g, c)?,
				"channel is already configured; edit it instead"
			);
		}
		("channels", "edit") => {
			let lang = field(a, "language");
			if !lang.is_empty() {
				crate::i18n::I18n::load(lang)?;
			}
		}
		("channels", "patterns") => {
			ensure!(
				matches!(field(a, "operation"), "add" | "remove"),
				"operation must be add or remove"
			);
			let (g, _) = channel_ids(a)?;
			ensure!(
				db.get_server_tags(g)?
					.contains(&field(a, "name").to_string()),
				"pattern must be enabled for this guild"
			);
		}
		("settings", "add" | "edit") => {
			crate::settings::validate(field(a, "key"), field(a, "value"))?
		}
		("moderators", "add") => {
			let g = field(a, "guild").parse::<i64>()?;
			let u = field(a, "user").parse::<i64>()?;
			ensure!(g > 0 && u > 0, "IDs must be positive");
			db.get_server(g)?.context("guild not known to bot")?;
		}
		_ => {}
	}
	Ok(())
}

fn apply(handler: &Handler, user: u64, guild: Option<i64>, a: &Action) -> Result<String> {
	authorize(handler, user, guild, a)?;
	validate_form(handler, guild, a)?;
	let db = handler.db.lock().expect("db mutex poisoned");
	match (a.category.as_str(), a.operation.as_str()) {
		("boorus", "add") => {
			db.add_booru(&new_provider(a)?)?;
		}
		("boorus", "edit") => {
			let mut row = provider(&db, a)?;
			super::admin::booru::edit_field(&mut row, field(a, "field"), field(a, "value"))?;
			db.update_booru(&row)?;
		}
		("boorus", "delete") => db.delete_booru(&provider(&db, a)?.name)?,
		("boorus", "toggle") => {
			let row = provider(&db, a)?;
			db.set_booru_enabled(&row.name, !row.enabled)?;
		}
		("boorus", "parameters") => {
			let row = provider(&db, a)?;
			if field(a, "operation") == "delete" {
				db.delete_booru_custom_parameter(&row.name, field(a, "key"))?;
			} else {
				db.set_booru_custom_parameter(&row.name, field(a, "key"), field(a, "value"))?;
			}
		}
		("patterns", "add") => db.add_tag_pattern(
			field(a, "name"),
			field(a, "booru"),
			&csv(field(a, "included")),
			&csv(field(a, "excluded")),
		)?,
		("patterns", "delete") => {
			let id = target(a)?.parse::<i64>()?;
			let p = db
				.get_tag_patterns(None)?
				.into_iter()
				.find(|p| p.id == id)
				.context("pattern no longer exists")?;
			let b = db
				.get_booru_by_id(p.booru_id)?
				.context("provider no longer exists")?;
			db.delete_tag_pattern(&p.name, Some(&b.name))?;
		}
		("server_tags", "add") => {
			db.add_server_tag(guild.context("guild required")?, field(a, "name"))?
		}
		("server_tags", "delete") => {
			db.remove_server_tag(guild.context("guild required")?, target(a)?)?
		}
		("channels", "add") => {
			db.set_channel_config(&ChannelConfig {
				guild_id: field(a, "guild").parse()?,
				channel_id: field(a, "channel").parse()?,
				language: None,
				banned_tags: Vec::new(),
			})?;
		}
		("channels", "edit") => {
			let (g, c) = channel_ids(a)?;
			db.set_channel_config(&ChannelConfig {
				guild_id: g,
				channel_id: c,
				language: if field(a, "language").is_empty() {
					None
				} else {
					Some(field(a, "language").trim().to_ascii_lowercase())
				},
				banned_tags: csv(field(a, "banned")),
			})?;
		}
		("channels", "delete") => {
			let (g, c) = channel_ids(a)?;
			db.remove_channel(g, c)?;
		}
		("channels", "patterns") => {
			let (g, c) = channel_ids(a)?;
			if field(a, "operation") == "add" {
				db.add_channel_pattern(g, c, field(a, "name"))?;
			} else {
				db.remove_channel_pattern(g, c, field(a, "name"))?;
			}
		}
		("settings", "add" | "edit") => {
			if a.operation == "edit" {
				ensure!(
					a.target.as_deref() == Some(field(a, "key")),
					"edit cannot rename a setting; add/delete instead"
				);
			}
			db.set_setting(field(a, "key"), field(a, "value"))?;
		}
		("settings", "delete") => db.delete_setting(target(a)?)?,
		("settings", "reset") => {
			let key = target(a)?;
			let value = crate::settings::DEFAULTS
				.iter()
				.find(|(k, _)| *k == key)
				.context("custom settings have no default; delete instead")?
				.1;
			db.set_setting(key, value)?;
		}
		("servers", "validate" | "unvalidate") => {
			db.set_validated(target(a)?.parse()?, a.operation == "validate")?
		}
		("moderators", "add") => db.add_moderator(
			field(a, "user").parse()?,
			Some(field(a, "guild").parse()?),
			user as i64,
		)?,
		("moderators", "delete") => {
			let (g, u) = channel_ids(a)?;
			db.remove_moderator(u, Some(g))?;
		}
		_ => anyhow::bail!("unknown configuration action"),
	}
	Ok("Saved. Runtime values apply immediately; slash commands are being reconciled.".into())
}

fn test_candidate(
	handler: &Handler,
	guild: Option<i64>,
	channel: i64,
	a: &Action,
) -> Result<(BooruConfig, Vec<String>, Vec<String>)> {
	let mut blacklist = handler.blacklist(guild, channel)?;
	let db = handler.db.lock().expect("db mutex poisoned");
	if a.category == "boorus" {
		let config = BooruConfig::from_row(&provider(&db, a)?)?;
		let tags = super::commands::test_tags(field(a, "tags"), &config)?;
		return Ok((config, tags, blacklist));
	}
	let p = db
		.get_tag_patterns(None)?
		.into_iter()
		.find(|p| Some(p.id.to_string().as_str()) == a.target.as_deref())
		.context("pattern no longer exists")?;
	let choices = db
		.get_tag_patterns(Some(&p.name))?
		.into_iter()
		.filter_map(|p| match db.get_booru_by_id(p.booru_id) {
			Ok(Some(b)) if b.enabled => Some(Ok((p, b))),
			Ok(_) => None,
			Err(err) => Some(Err(err)),
		})
		.collect::<Result<Vec<_>>>()?;
	ensure!(!choices.is_empty(), "no enabled provider for this pattern");
	let (pattern, row) = &choices[rand::rng().random_range(0..choices.len())];
	let mut tags = Vec::new();
	for e in db.get_pattern_entries(pattern.id)? {
		if e.is_excluded {
			blacklist.push(e.tag);
		} else {
			tags.push(e.tag);
		}
	}
	Ok((BooruConfig::from_row(row)?, tags, blacklist))
}

fn truncate(value: &str, max: usize) -> String {
	if value.chars().count() <= max {
		value.into()
	} else if max < 3 {
		value.chars().take(max).collect()
	} else {
		format!("{}...", value.chars().take(max - 3).collect::<String>())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn permission_matrix() {
		assert!(check_scope(true, false, Some(1), "settings", Some(2)).is_ok());
		assert!(check_scope(false, true, Some(1), "channels", Some(1)).is_ok());
		assert!(check_scope(false, true, Some(1), "channels", Some(2)).is_err());
		assert!(check_scope(false, true, Some(1), "boorus", None).is_err());
		assert!(check_scope(false, false, Some(1), "main", None).is_err());
	}
	#[test]
	fn targets_are_user_guild_bound_and_opaque() {
		let store = PanelStore::default();
		let id = store.put(
			1,
			Some(10),
			Action::new("settings", "delete", Stage::Confirm).target("api:key"),
		);
		assert!(!id.contains("api:key"));
		assert!(id.len() < 100);
		assert!(store.get(&id, 2, Some(10), false).is_err());
		assert!(store.get(&id, 1, Some(11), false).is_err());
		assert_eq!(
			store.get(&id, 1, Some(10), true).unwrap().target.as_deref(),
			Some("api:key")
		);
		assert!(store.get(&id, 1, Some(10), false).is_err());
	}
	#[test]
	fn csv_fields_do_not_become_command_arguments() {
		assert_eq!(csv("glasses, solo"), vec!["glasses", "solo"]);
	}
	#[test]
	fn unicode_truncation_is_safe() {
		assert!(
			truncate(&format!("{}éx", "a".repeat(3899)), 3900)
				.chars()
				.count() <= 3900
		);
	}
}
