use serenity::all::{
	ButtonStyle, ComponentInteraction, CreateActionRow, CreateButton, CreateEmbed, CreateInputText,
	CreateInteractionResponse, CreateInteractionResponseMessage, CreateModal,
	EditInteractionResponse, InputTextStyle, ModalInteraction, ReactionType,
};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::Handler;

const EMBED_COLOR: u32 = 0x5865F2;

pub(crate) async fn handle_administrate_embed(
	handler: &Handler,
	command: &serenity::all::CommandInteraction,
	ctx: &serenity::all::Context,
	user_id: i64,
) {
	let is_admin = user_id == handler.admin_user_id as i64;
	let is_mod = {
		let db = handler.db.lock().expect("db mutex poisoned");
		let guild_id = command.guild_id.map(|g| g.get() as i64);
		db.is_moderator(user_id, guild_id).unwrap_or(false)
	};
	if !is_admin && !is_mod {
		handler
			.respond(&ctx.http, command, handler.i18n.admin_only())
			.await;
		return;
	}

	let (embed, rows) =
		build_main_menu(handler, is_admin, command.guild_id.map(|g| g.get() as i64));
	if command.defer(&ctx.http).await.is_ok() {
		let _ = command
			.edit_response(
				&ctx.http,
				EditInteractionResponse::new().embed(embed).components(rows),
			)
			.await;
	}
}

pub(crate) async fn handle_component_interaction(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
) {
	let custom_id = &interaction.data.custom_id;
	let user_id = interaction.user.id.get() as i64;
	let is_admin = user_id == handler.admin_user_id as i64;

	if let Some(category) = custom_id.strip_prefix("ac:") {
		let (embed, rows) = build_category_page(
			handler,
			category,
			is_admin,
			interaction.guild_id.map(|g| g.get() as i64),
		);
		let _ = interaction
			.create_response(
				&ctx.http,
				CreateInteractionResponse::UpdateMessage(
					CreateInteractionResponseMessage::new()
						.embed(embed)
						.components(rows),
				),
			)
			.await;
		return;
	}

	if let Some(action_str) = custom_id.strip_prefix("aa:") {
		handle_action_button(handler, interaction, ctx, action_str, is_admin).await;
		return;
	}

	if let Some(action_str) = custom_id.strip_prefix("as:") {
		handle_target_button(handler, interaction, ctx, action_str, is_admin).await;
		return;
	}

	if let Some(action_str) = custom_id.strip_prefix("ay:") {
		handle_confirmed_action(handler, interaction, ctx, action_str, is_admin).await;
		return;
	}

	if custom_id == "an" {
		let (embed, rows) = build_main_menu(
			handler,
			is_admin,
			interaction.guild_id.map(|g| g.get() as i64),
		);
		let _ = interaction
			.create_response(
				&ctx.http,
				CreateInteractionResponse::UpdateMessage(
					CreateInteractionResponseMessage::new()
						.embed(embed)
						.components(rows),
				),
			)
			.await;
	}
}

pub(crate) async fn handle_modal_interaction(
	handler: &Handler,
	interaction: &ModalInteraction,
	ctx: &serenity::all::Context,
) {
	let custom_id = &interaction.data.custom_id;
	let is_admin = interaction.user.id.get() as i64 == handler.admin_user_id as i64;

	let _ = interaction
		.create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
		.await;

	let result = process_modal_submission(
		handler,
		custom_id,
		&interaction.data.components,
		interaction.guild_id.map(|g| g.get() as i64),
	)
	.await;

	let (embed, rows) = if custom_id.starts_with("am:") {
		let category = extract_category_from_modal_id(custom_id);
		build_category_page(
			handler,
			category,
			is_admin,
			interaction.guild_id.map(|g| g.get() as i64),
		)
	} else {
		build_main_menu(
			handler,
			is_admin,
			interaction.guild_id.map(|g| g.get() as i64),
		)
	};

	let content = match result {
		Ok(msg) => msg,
		Err(err) => format!("Error: {err}"),
	};

	let _ = interaction
		.edit_response(
			&ctx.http,
			EditInteractionResponse::new()
				.content(content)
				.embed(embed)
				.components(rows),
		)
		.await;
}

fn extract_category_from_modal_id(custom_id: &str) -> &str {
	custom_id
		.strip_prefix("am:")
		.and_then(|rest| rest.split(':').next())
		.unwrap_or("main")
}

fn build_main_menu(
	handler: &Handler,
	_is_admin: bool,
	current_guild: Option<i64>,
) -> (CreateEmbed, Vec<CreateActionRow>) {
	let db = handler.db.lock().expect("db mutex poisoned");
	let booru_count = db.get_all_boorus().map(|b| b.len()).unwrap_or(0);
	let enabled_count = db.get_enabled_boorus().map(|b| b.len()).unwrap_or(0);
	let tag_count = db.get_unique_pattern_names().map(|p| p.len()).unwrap_or(0);
	let server_count = db.get_all_servers().map(|s| s.len()).unwrap_or(0);
	let validated_count = db.get_validated_servers().map(|s| s.len()).unwrap_or(0);

	let embed = CreateEmbed::new()
		.title("Administration Panel")
		.description("Select a category to manage.")
		.field(
			"Boorus",
			format!("{booru_count} total, {enabled_count} enabled"),
			true,
		)
		.field("Tags", format!("{tag_count} command names"), true)
		.field(
			"Servers",
			format!("{server_count} total, {validated_count} validated"),
			true,
		)
		.color(EMBED_COLOR);

	let first_row = vec![
		CreateButton::new("ac:tags")
			.label("Tags")
			.style(ButtonStyle::Primary)
			.emoji(ReactionType::Unicode("🏷️".to_string())),
		CreateButton::new("ac:server_tags")
			.label("Server Tags")
			.style(ButtonStyle::Primary)
			.emoji(ReactionType::Unicode("🗂️".to_string())),
		CreateButton::new("ac:boorus")
			.label("Boorus")
			.style(ButtonStyle::Primary)
			.emoji(ReactionType::Unicode("🖼️".to_string())),
		CreateButton::new("ac:channels")
			.label("Channels")
			.style(ButtonStyle::Primary)
			.emoji(ReactionType::Unicode("📢".to_string())),
		CreateButton::new("ac:settings")
			.label("Settings")
			.style(ButtonStyle::Primary)
			.emoji(ReactionType::Unicode("⚙️".to_string())),
	];

	let show_validate = current_guild
		.and_then(|guild_id| db.get_server(guild_id).ok().flatten())
		.is_some_and(|server| !server.validated);
	let mut rows = vec![CreateActionRow::Buttons(first_row)];

	let reload_row = vec![
		CreateButton::new("aa:reload")
			.label("Reload")
			.style(ButtonStyle::Danger)
			.emoji(ReactionType::Unicode("🔄".to_string())),
	];
	rows.push(CreateActionRow::Buttons(reload_row));

	if show_validate {
		rows.push(CreateActionRow::Buttons(vec![
			CreateButton::new("ac:validate")
				.label("Validate")
				.style(ButtonStyle::Success)
				.emoji(ReactionType::Unicode("✅".to_string())),
		]));
	}

	(embed, rows)
}

fn build_category_page(
	handler: &Handler,
	category: &str,
	is_admin: bool,
	current_guild: Option<i64>,
) -> (CreateEmbed, Vec<CreateActionRow>) {
	match category {
		"tags" => build_tags_page(handler, is_admin),
		"server_tags" => build_server_tags_page(handler, is_admin, current_guild),
		"boorus" => build_boorus_page(handler, is_admin),
		"channels" => build_channels_page(handler, is_admin),
		"validate" => build_validate_page(handler, is_admin),
		"settings" => build_settings_page(handler, is_admin),
		_ => build_main_menu(handler, is_admin, current_guild),
	}
}

fn build_tags_page(handler: &Handler, _is_admin: bool) -> (CreateEmbed, Vec<CreateActionRow>) {
	let db = handler.db.lock().expect("db mutex poisoned");
	let mut description = String::new();

	match db.get_all_boorus() {
		Ok(boorus) => {
			if boorus.is_empty() {
				description.push_str("No boorus configured.");
			} else {
				for booru in &boorus {
					let patterns = db.get_tag_patterns(None).unwrap_or_default();
					let booru_patterns: Vec<_> =
						patterns.iter().filter(|p| p.booru_id == booru.id).collect();
					if booru_patterns.is_empty() {
						continue;
					}
					description.push_str(&format!("**{}:**\n", booru.name));
					for pattern in &booru_patterns {
						let entries = db.get_pattern_entries(pattern.id).unwrap_or_default();
						let tags: Vec<&str> = entries
							.iter()
							.filter(|e| !e.is_excluded)
							.map(|e| e.tag.as_str())
							.collect();
						description.push_str(&format!(
							"  {} ({})\n",
							pattern.name,
							tags.join(", ")
						));
					}
				}
				if description.is_empty() {
					description.push_str("No tag patterns assigned to any booru.");
				}
			}
		}
		Err(err) => {
			description = format!("Error: {err}");
		}
	}

	let embed = CreateEmbed::new()
		.title("🏷️ Tags")
		.description(truncate(&description, 3900))
		.color(EMBED_COLOR);

	let buttons = vec![
		CreateButton::new("aa:tag:add")
			.label("Add Tags")
			.style(ButtonStyle::Success)
			.emoji(ReactionType::Unicode("➕".to_string())),
		CreateButton::new("aa:tag:remove")
			.label("Remove Tags")
			.style(ButtonStyle::Danger)
			.emoji(ReactionType::Unicode("🗑️".to_string())),
		CreateButton::new("ac:main")
			.label("Back")
			.style(ButtonStyle::Secondary)
			.emoji(ReactionType::Unicode("↩️".to_string())),
	];

	(embed, vec![CreateActionRow::Buttons(buttons)])
}

fn build_boorus_page(handler: &Handler, _is_admin: bool) -> (CreateEmbed, Vec<CreateActionRow>) {
	let db = handler.db.lock().expect("db mutex poisoned");
	let mut description = String::new();

	match db.get_all_boorus() {
		Ok(boorus) => {
			if boorus.is_empty() {
				description.push_str("No boorus configured.");
			} else {
				for booru in &boorus {
					let status = if booru.enabled { "🟢" } else { "🔴" };
					let desc = if booru.description.is_empty() {
						String::new()
					} else {
						format!(" - {}", booru.description)
					};
					description.push_str(&format!(
						"{} **{}**{}  (page_size: {}, max_tags: {})\n",
						status, booru.name, desc, booru.page_size, booru.max_tags
					));
				}
			}
		}
		Err(err) => {
			description = format!("Error: {err}");
		}
	}

	let embed = CreateEmbed::new()
		.title("🖼️ Boorus")
		.description(truncate(&description, 3900))
		.color(EMBED_COLOR);

	let buttons = vec![
		CreateButton::new("aa:booru:add")
			.label("Add")
			.style(ButtonStyle::Success)
			.emoji(ReactionType::Unicode("➕".to_string())),
		CreateButton::new("aa:booru:edit")
			.label("Edit")
			.style(ButtonStyle::Primary)
			.emoji(ReactionType::Unicode("✏️".to_string())),
		CreateButton::new("aa:booru:delete")
			.label("Delete")
			.style(ButtonStyle::Danger)
			.emoji(ReactionType::Unicode("🗑️".to_string())),
		CreateButton::new("aa:booru:toggle")
			.label("Toggle")
			.style(ButtonStyle::Secondary)
			.emoji(ReactionType::Unicode("🔄".to_string())),
		CreateButton::new("aa:booru:param")
			.label("Parameters")
			.style(ButtonStyle::Secondary)
			.emoji(ReactionType::Unicode("⚙️".to_string())),
	];

	let back_row = vec![
		CreateButton::new("ac:main")
			.label("Back")
			.style(ButtonStyle::Secondary)
			.emoji(ReactionType::Unicode("↩️".to_string())),
	];

	(
		embed,
		vec![
			CreateActionRow::Buttons(buttons),
			CreateActionRow::Buttons(back_row),
		],
	)
}

fn build_channels_page(handler: &Handler, _is_admin: bool) -> (CreateEmbed, Vec<CreateActionRow>) {
	let db = handler.db.lock().expect("db mutex poisoned");
	let mut description = String::new();

	match db.get_all_servers() {
		Ok(servers) => {
			for server in &servers {
				let channels = db.get_guild_channels(server.guild_id).unwrap_or_default();
				if channels.is_empty() {
					continue;
				}
				description.push_str(&format!("**{}** ({}):\n", server.name, server.guild_id));
				for channel in &channels {
					let lang = channel.language.as_deref().unwrap_or("default");
					let patterns = db
						.get_channel_patterns(channel.guild_id, channel.channel_id)
						.unwrap_or_default();
					description.push_str(&format!(
						"  <#{}> lang={} banned={:?} tags={}\n",
						channel.channel_id,
						lang,
						channel.banned_tags,
						if patterns.is_empty() {
							"none".to_string()
						} else {
							patterns.join(", ")
						}
					));
				}
			}
			if description.is_empty() {
				description.push_str("No channels configured.");
			}
		}
		Err(err) => {
			description = format!("Error: {err}");
		}
	}

	let embed = CreateEmbed::new()
		.title("📢 Channels")
		.description(truncate(&description, 3900))
		.color(EMBED_COLOR);

	let buttons = vec![
		CreateButton::new("aa:ch:add")
			.label("Add Channel")
			.style(ButtonStyle::Success)
			.emoji(ReactionType::Unicode("➕".to_string())),
		CreateButton::new("aa:ch:remove")
			.label("Remove Channel")
			.style(ButtonStyle::Danger)
			.emoji(ReactionType::Unicode("🗑️".to_string())),
		CreateButton::new("aa:ch:set")
			.label("Set Config")
			.style(ButtonStyle::Primary)
			.emoji(ReactionType::Unicode("✏️".to_string())),
		CreateButton::new("aa:ch:pattern")
			.label("Patterns")
			.style(ButtonStyle::Secondary)
			.emoji(ReactionType::Unicode("📋".to_string())),
	];

	let back_row = vec![
		CreateButton::new("ac:main")
			.label("Back")
			.style(ButtonStyle::Secondary)
			.emoji(ReactionType::Unicode("↩️".to_string())),
	];

	(
		embed,
		vec![
			CreateActionRow::Buttons(buttons),
			CreateActionRow::Buttons(back_row),
		],
	)
}

fn build_server_tags_page(
	handler: &Handler,
	_is_admin: bool,
	current_guild: Option<i64>,
) -> (CreateEmbed, Vec<CreateActionRow>) {
	let Some(guild_id) = current_guild else {
		let embed = CreateEmbed::new()
			.title("🗂️ Server Tags")
			.description("Run this panel from inside a server channel.")
			.color(EMBED_COLOR);
		return (embed, Vec::new());
	};

	let db = handler.db.lock().expect("db mutex poisoned");
	let tags = db.get_server_tags(guild_id).unwrap_or_default();
	let mut description = String::new();
	if tags.is_empty() {
		description.push_str(
			"No tag commands enabled for this server yet.\nAdd tag names to register slash commands here.",
		);
	} else {
		description.push_str("Enabled tag commands:\n");
		for tag in &tags {
			description.push_str(&format!("  • {tag}\n"));
		}
	}
	let embed = CreateEmbed::new()
		.title("🗂️ Server Tags")
		.description(truncate(&description, 3900))
		.footer(serenity::all::CreateEmbedFooter::new(format!(
			"Server ID: {guild_id}"
		)))
		.color(EMBED_COLOR);

	let buttons = vec![
		CreateButton::new("aa:srvtag:add")
			.label("Add Tag")
			.style(ButtonStyle::Success)
			.emoji(ReactionType::Unicode("➕".to_string())),
		CreateButton::new("aa:srvtag:remove")
			.label("Remove Tag")
			.style(ButtonStyle::Danger)
			.emoji(ReactionType::Unicode("🗑️".to_string())),
		CreateButton::new("ac:main")
			.label("Back")
			.style(ButtonStyle::Secondary)
			.emoji(ReactionType::Unicode("↩️".to_string())),
	];

	(embed, vec![CreateActionRow::Buttons(buttons)])
}

fn build_settings_page(handler: &Handler, _is_admin: bool) -> (CreateEmbed, Vec<CreateActionRow>) {
	let db = handler.db.lock().expect("db mutex poisoned");
	let mut description = String::new();

	match db.get_all_settings() {
		Ok(settings) => {
			if settings.is_empty() {
				description.push_str("No settings configured. Defaults are active.");
			} else {
				for (key, value) in settings {
					description.push_str(&format!("**{key}** = `{value}`\n"));
				}
			}
		}
		Err(err) => description = format!("Error: {err}"),
	}

	let embed = CreateEmbed::new()
		.title("⚙️ Settings")
		.description(truncate(&description, 3900))
		.color(EMBED_COLOR);
	let buttons = vec![
		CreateButton::new("aa:settings:add")
			.label("Add")
			.style(ButtonStyle::Success)
			.emoji(ReactionType::Unicode("➕".to_string())),
		CreateButton::new("aa:settings:edit")
			.label("Edit")
			.style(ButtonStyle::Primary)
			.emoji(ReactionType::Unicode("✏️".to_string())),
		CreateButton::new("aa:settings:delete")
			.label("Delete")
			.style(ButtonStyle::Danger)
			.emoji(ReactionType::Unicode("🗑️".to_string())),
		CreateButton::new("ac:main")
			.label("Back")
			.style(ButtonStyle::Secondary)
			.emoji(ReactionType::Unicode("↩️".to_string())),
	];

	(embed, vec![CreateActionRow::Buttons(buttons)])
}

fn build_validate_page(handler: &Handler, is_admin: bool) -> (CreateEmbed, Vec<CreateActionRow>) {
	let db = handler.db.lock().expect("db mutex poisoned");
	let mut description = String::new();

	match db.get_all_servers() {
		Ok(servers) => {
			let unvalidated: Vec<_> = servers.iter().filter(|s| !s.validated).collect();
			let validated: Vec<_> = servers.iter().filter(|s| s.validated).collect();

			if !validated.is_empty() {
				description.push_str("**Validated:**\n");
				for server in &validated {
					description.push_str(&format!("  ✅ {} ({})\n", server.name, server.guild_id));
				}
			}
			if !unvalidated.is_empty() {
				description.push_str("**Pending:**\n");
				for server in &unvalidated {
					description.push_str(&format!("  ⏳ {} ({})\n", server.name, server.guild_id));
				}
			}
			if servers.is_empty() {
				description.push_str("No servers found.");
			}
		}
		Err(err) => {
			description = format!("Error: {err}");
		}
	}

	let embed = CreateEmbed::new()
		.title("✅ Server Validation")
		.description(truncate(&description, 3900))
		.color(EMBED_COLOR);

	let mut buttons = Vec::new();
	if is_admin {
		buttons.push(
			CreateButton::new("aa:val:validate")
				.label("Validate Server")
				.style(ButtonStyle::Success)
				.emoji(ReactionType::Unicode("✅".to_string())),
		);
		buttons.push(
			CreateButton::new("aa:val:unvalidate")
				.label("Unvalidate Server")
				.style(ButtonStyle::Danger)
				.emoji(ReactionType::Unicode("❌".to_string())),
		);
	}
	buttons.push(
		CreateButton::new("ac:main")
			.label("Back")
			.style(ButtonStyle::Secondary)
			.emoji(ReactionType::Unicode("↩️".to_string())),
	);

	(embed, vec![CreateActionRow::Buttons(buttons)])
}

async fn handle_action_button(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	action_str: &str,
	is_admin: bool,
) {
	let parts: Vec<&str> = action_str.splitn(2, ':').collect();
	let category = parts[0];
	let action = parts.get(1).copied().unwrap_or("");

	match (category, action) {
		("booru", "add") => {
			let modal = CreateModal::new("am:boorus:add", "Add Booru").components(vec![
				CreateActionRow::InputText(
					CreateInputText::new(InputTextStyle::Short, "Booru Name", "booru_name")
						.required(true)
						.placeholder("e.g. danbooru"),
				),
				CreateActionRow::InputText(
					CreateInputText::new(InputTextStyle::Short, "Description", "description")
						.required(false)
						.placeholder("Optional slash command description"),
				),
				CreateActionRow::InputText(
					CreateInputText::new(InputTextStyle::Paragraph, "API URLs JSON", "urls_json")
						.required(true)
						.placeholder(
							"{\"posts_url\":\"...\",\"count_url\":null,\"post_url\":\"...\"}",
						),
				),
				CreateActionRow::InputText(
					CreateInputText::new(
						InputTextStyle::Paragraph,
						"JSON Paths JSON",
						"paths_json",
					)
					.required(true)
					.placeholder(
						"{\"posts_path\":[\"posts\"],\"file_url_path\":[\"file_url\"],...}",
					),
				),
				CreateActionRow::InputText(
					CreateInputText::new(InputTextStyle::Paragraph, "Options JSON", "options_json")
						.required(true)
						.placeholder(
							"{\"embed_image\":false,\"max_tags\":6,\"page_size\":100,...}",
						),
				),
			]);
			let _ = interaction
				.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
				.await;
		}
		("booru", "edit") => {
			show_booru_targets(handler, interaction, ctx, "edit", "Select Booru to Edit").await
		}
		("booru", "delete") => {
			show_booru_targets(
				handler,
				interaction,
				ctx,
				"delete",
				"Select Booru to Delete",
			)
			.await;
		}
		("booru", "toggle") => {
			show_booru_targets(
				handler,
				interaction,
				ctx,
				"toggle",
				"Select Booru to Toggle",
			)
			.await;
		}
		("booru", "param") => {
			show_booru_targets(
				handler,
				interaction,
				ctx,
				"param",
				"Select Booru Parameters",
			)
			.await;
		}
		("tag", "add") => {
			let modal = CreateModal::new("am:tags:add", "Add Tags to Pattern").components(vec![
				CreateActionRow::InputText(
					CreateInputText::new(InputTextStyle::Short, "Pattern Name", "pattern_name")
						.required(true),
				),
				CreateActionRow::InputText(
					CreateInputText::new(InputTextStyle::Short, "Booru Name", "booru_name")
						.required(true),
				),
				CreateActionRow::InputText(
					CreateInputText::new(
						InputTextStyle::Short,
						"Included Tags (comma-separated)",
						"included_tags",
					)
					.required(true),
				),
				CreateActionRow::InputText(
					CreateInputText::new(
						InputTextStyle::Short,
						"Excluded Tags (comma-separated)",
						"excluded_tags",
					)
					.required(false),
				),
			]);
			let _ = interaction
				.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
				.await;
		}
		("tag", "remove") => {
			show_pattern_targets(handler, interaction, ctx, "tags", "Select Tags to Remove").await;
		}
		("ch", "add") => {
			let modal = CreateModal::new("am:channels:add", "Add Channel").components(vec![
				CreateActionRow::InputText(
					CreateInputText::new(
						InputTextStyle::Short,
						"Guild ID (leave empty for current)",
						"guild_id",
					)
					.required(false),
				),
				CreateActionRow::InputText(
					CreateInputText::new(InputTextStyle::Short, "Channel ID", "channel_id")
						.required(true),
				),
			]);
			let _ = interaction
				.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
				.await;
		}
		("ch", "remove") => {
			show_channel_targets(
				handler,
				interaction,
				ctx,
				"remove",
				"Select Channel to Remove",
			)
			.await;
		}
		("ch", "set") => {
			show_channel_targets(handler, interaction, ctx, "set", "Select Channel to Edit").await;
		}
		("ch", "pattern") => {
			show_channel_targets(
				handler,
				interaction,
				ctx,
				"pattern",
				"Select Channel Patterns",
			)
			.await;
		}
		("settings", "add") => {
			let modal = setting_modal("am:settings:set", "Set/Edit Setting", "", "");
			let _ = interaction
				.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
				.await;
		}
		("settings", "edit") => {
			show_setting_targets(handler, interaction, ctx, "edit", "Select Setting to Edit").await;
		}
		("settings", "delete") => {
			show_setting_targets(
				handler,
				interaction,
				ctx,
				"delete",
				"Select Setting to Delete",
			)
			.await;
		}
		("srvtag", "add") => {
			let modal = CreateModal::new("am:server_tags:add", "Add Server Tag").components(vec![
				CreateActionRow::InputText(
					CreateInputText::new(InputTextStyle::Short, "Tag Name", "tag_name")
						.required(true)
						.placeholder("Must match a configured tag pattern"),
				),
			]);
			let _ = interaction
				.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
				.await;
		}
		("srvtag", "remove") => {
			show_server_tag_targets(
				handler,
				interaction,
				ctx,
				"remove",
				"Select Server Tag to Remove",
			)
			.await;
		}
		("reload", "") => {
			handle_reload_component(handler, interaction, ctx).await;
		}
		("val", "validate") if is_admin => {
			let modal =
				CreateModal::new("am:validate:validate", "Validate Server").components(vec![
					CreateActionRow::InputText(
						CreateInputText::new(InputTextStyle::Short, "Guild ID", "guild_id")
							.required(true),
					),
				]);
			let _ = interaction
				.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
				.await;
		}
		("val", "unvalidate") if is_admin => {
			let modal =
				CreateModal::new("am:validate:unvalidate", "Unvalidate Server").components(vec![
					CreateActionRow::InputText(
						CreateInputText::new(InputTextStyle::Short, "Guild ID", "guild_id")
							.required(true),
					),
				]);
			let _ = interaction
				.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
				.await;
		}
		_ => {
			let _ = interaction
				.create_response(
					&ctx.http,
					CreateInteractionResponse::Message(
						CreateInteractionResponseMessage::new()
							.content("Action not available.")
							.ephemeral(true),
					),
				)
				.await;
		}
	}
}

async fn handle_target_button(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	action_str: &str,
	is_admin: bool,
) {
	let parts: Vec<&str> = action_str.split(':').collect();
	match parts.as_slice() {
		["boorus", "edit", id] => show_booru_edit_modal(handler, interaction, ctx, id).await,
		["boorus", "delete", id] => {
			if let Some(name) = booru_name(handler, id) {
				show_confirmation(
					interaction,
					ctx,
					&format!("Delete booru `{name}`? This also removes its patterns."),
					&format!("booru:delete:{id}"),
				)
				.await;
			}
		}
		["boorus", "toggle", id] => {
			if let Some(name) = booru_name(handler, id) {
				show_confirmation(
					interaction,
					ctx,
					&format!("Toggle booru `{name}` enabled status?"),
					&format!("booru:toggle:{id}"),
				)
				.await;
			}
		}
		["boorus", "param", id] => show_booru_param_modal(handler, interaction, ctx, id).await,
		["patterns", category, id] => {
			if let Some(label) = pattern_label(handler, id) {
				show_confirmation(
					interaction,
					ctx,
					&format!("Remove `{label}`?"),
					&format!("pattern:delete:{category}:{id}"),
				)
				.await;
			}
		}
		["channels", "remove", guild_id, channel_id] => {
			show_confirmation(
				interaction,
				ctx,
				&format!("Remove channel <#{channel_id}> from server `{guild_id}`?"),
				&format!("channel:remove:{guild_id}:{channel_id}"),
			)
			.await;
		}
		["channels", "set", guild_id, channel_id] => {
			show_channel_set_modal(handler, interaction, ctx, guild_id, channel_id).await
		}
		["channels", "pattern", guild_id, channel_id] => {
			show_channel_pattern_modal(interaction, ctx, guild_id, channel_id).await
		}
		["settings", "delete", key] => {
			show_confirmation(
				interaction,
				ctx,
				&format!("Delete setting `{key}`?"),
				&format!("settings:delete:{key}"),
			)
			.await;
		}
		["settings", "edit", key] => show_setting_edit_modal(handler, interaction, ctx, key).await,
		["server_tags", "remove", tag] => {
			show_confirmation(
				interaction,
				ctx,
				&format!("Remove tag `{tag}` from this server?"),
				&format!("server_tag:remove:{tag}"),
			)
			.await;
		}
		_ => {
			let (embed, rows) = build_main_menu(
				handler,
				is_admin,
				interaction.guild_id.map(|g| g.get() as i64),
			);
			let _ = interaction
				.create_response(
					&ctx.http,
					CreateInteractionResponse::UpdateMessage(
						CreateInteractionResponseMessage::new()
							.embed(embed)
							.components(rows),
					),
				)
				.await;
		}
	}
}

async fn show_booru_targets(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	action: &str,
	title: &str,
) {
	let items = {
		let db = handler.db.lock().expect("db mutex poisoned");
		db.get_all_boorus()
			.unwrap_or_default()
			.into_iter()
			.map(|booru| (format!("as:boorus:{action}:{}", booru.id), booru.name))
			.collect::<Vec<_>>()
	};
	show_target_page(
		interaction,
		ctx,
		title,
		"Choose one booru before applying the action.",
		items,
		"ac:boorus",
	)
	.await;
}

async fn show_pattern_targets(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	category: &str,
	title: &str,
) {
	let items = {
		let db = handler.db.lock().expect("db mutex poisoned");
		db.get_tag_patterns(None)
			.unwrap_or_default()
			.into_iter()
			.map(|pattern| {
				let booru = db
					.get_booru_by_id(pattern.booru_id)
					.ok()
					.flatten()
					.map(|booru| booru.name)
					.unwrap_or_else(|| "unknown".to_string());
				(
					format!("as:patterns:{category}:{}", pattern.id),
					format!("{} / {booru}", pattern.name),
				)
			})
			.collect::<Vec<_>>()
	};
	show_target_page(
		interaction,
		ctx,
		title,
		"Choose one existing pattern before removing it.",
		items,
		&format!("ac:{category}"),
	)
	.await;
}

async fn show_channel_targets(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	action: &str,
	title: &str,
) {
	let items = {
		let db = handler.db.lock().expect("db mutex poisoned");
		let mut items = Vec::new();
		for server in db.get_all_servers().unwrap_or_default() {
			for channel in db.get_guild_channels(server.guild_id).unwrap_or_default() {
				items.push((
					format!(
						"as:channels:{action}:{}:{}",
						channel.guild_id, channel.channel_id
					),
					format!("{} / #{}", server.name, channel.channel_id),
				));
			}
		}
		items
	};
	show_target_page(
		interaction,
		ctx,
		title,
		"Choose one configured channel before applying the action.",
		items,
		"ac:channels",
	)
	.await;
}

async fn show_setting_targets(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	action: &str,
	title: &str,
) {
	let items = {
		let db = handler.db.lock().expect("db mutex poisoned");
		db.get_all_settings()
			.unwrap_or_default()
			.into_iter()
			.map(|(key, _)| (format!("as:settings:{action}:{key}"), key))
			.collect::<Vec<_>>()
	};
	show_target_page(
		interaction,
		ctx,
		title,
		"Choose one setting before applying the action.",
		items,
		"ac:settings",
	)
	.await;
}

async fn show_server_tag_targets(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	action: &str,
	title: &str,
) {
	let Some(guild_id) = interaction.guild_id.map(|g| g.get() as i64) else {
		return;
	};
	let items = {
		let db = handler.db.lock().expect("db mutex poisoned");
		db.get_server_tags(guild_id)
			.unwrap_or_default()
			.into_iter()
			.map(|tag| (format!("as:server_tags:{action}:{tag}"), tag))
			.collect::<Vec<_>>()
	};
	show_target_page(
		interaction,
		ctx,
		title,
		"Choose one tag enabled on this server.",
		items,
		"ac:server_tags",
	)
	.await;
}

async fn show_setting_edit_modal(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	key: &str,
) {
	let value = handler
		.db
		.lock()
		.expect("db mutex poisoned")
		.get_setting(key)
		.ok()
		.flatten()
		.unwrap_or_default();
	let modal = setting_modal("am:settings:set", format!("Edit {key}"), key, &value);
	let _ = interaction
		.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
		.await;
}

async fn show_target_page(
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	title: &str,
	description: &str,
	items: Vec<(String, String)>,
	back_id: &str,
) {
	let mut text = description.to_string();
	if items.is_empty() {
		text.push_str("\nNo items available.");
	}
	let mut rows = Vec::new();
	for chunk in items.chunks(5).take(4) {
		rows.push(CreateActionRow::Buttons(
			chunk
				.iter()
				.map(|(id, label)| {
					CreateButton::new(id.clone())
						.label(short_label(label))
						.style(ButtonStyle::Primary)
				})
				.collect(),
		));
	}
	rows.push(CreateActionRow::Buttons(vec![
		CreateButton::new(back_id)
			.label("Back")
			.style(ButtonStyle::Secondary)
			.emoji(ReactionType::Unicode("↩️".to_string())),
	]));
	let embed = CreateEmbed::new()
		.title(title)
		.description(text)
		.color(EMBED_COLOR);
	let _ = interaction
		.create_response(
			&ctx.http,
			CreateInteractionResponse::UpdateMessage(
				CreateInteractionResponseMessage::new()
					.embed(embed)
					.components(rows),
			),
		)
		.await;
}

async fn show_booru_edit_modal(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	id: &str,
) {
	let Some(name) = booru_name(handler, id) else {
		return;
	};
	let modal = CreateModal::new(format!("am:boorus:edit:{id}"), format!("Edit {name}"))
		.components(vec![
			CreateActionRow::InputText(
				CreateInputText::new(InputTextStyle::Short, "Field Name", "field_name")
					.required(true)
					.placeholder("description, page_size, count_url, enabled, ..."),
			),
			CreateActionRow::InputText(
				CreateInputText::new(InputTextStyle::Paragraph, "Value", "field_value")
					.required(true)
					.placeholder("Use null for optional fields."),
			),
		]);
	let _ = interaction
		.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
		.await;
}

async fn show_booru_param_modal(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	id: &str,
) {
	let Some(name) = booru_name(handler, id) else {
		return;
	};
	let modal = CreateModal::new(
		format!("am:boorus:param:{id}"),
		format!("Parameters for {name}"),
	)
	.components(vec![
		CreateActionRow::InputText(
			CreateInputText::new(InputTextStyle::Short, "Action", "param_action")
				.required(true)
				.placeholder("set, delete, or list"),
		),
		CreateActionRow::InputText(
			CreateInputText::new(InputTextStyle::Short, "Key", "param_key").required(false),
		),
		CreateActionRow::InputText(
			CreateInputText::new(InputTextStyle::Short, "Value", "param_value").required(false),
		),
	]);
	let _ = interaction
		.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
		.await;
}

async fn show_channel_set_modal(
	_handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	guild_id: &str,
	channel_id: &str,
) {
	let modal = CreateModal::new(
		format!("am:channels:set:{guild_id}:{channel_id}"),
		"Set Channel Config",
	)
	.components(vec![
		CreateActionRow::InputText(
			CreateInputText::new(InputTextStyle::Short, "Language", "language")
				.required(false)
				.placeholder("Leave empty for default"),
		),
		CreateActionRow::InputText(
			CreateInputText::new(InputTextStyle::Short, "Banned Tags", "banned_tags")
				.required(false)
				.placeholder("comma,separated,tags"),
		),
	]);
	let _ = interaction
		.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
		.await;
}

async fn show_channel_pattern_modal(
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	guild_id: &str,
	channel_id: &str,
) {
	let modal = CreateModal::new(
		format!("am:channels:pattern:{guild_id}:{channel_id}"),
		"Channel Patterns",
	)
	.components(vec![
		CreateActionRow::InputText(
			CreateInputText::new(InputTextStyle::Short, "Action", "pattern_action")
				.required(true)
				.placeholder("add, remove, or list"),
		),
		CreateActionRow::InputText(
			CreateInputText::new(InputTextStyle::Short, "Pattern Name", "pattern_name")
				.required(false),
		),
	]);
	let _ = interaction
		.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
		.await;
}

fn booru_name(handler: &Handler, id: &str) -> Option<String> {
	let id = id.parse::<i64>().ok()?;
	Some(
		handler
			.db
			.lock()
			.expect("db mutex poisoned")
			.get_booru_by_id(id)
			.ok()??
			.name,
	)
}

fn pattern_label(handler: &Handler, id: &str) -> Option<String> {
	let id = id.parse::<i64>().ok()?;
	let db = handler.db.lock().expect("db mutex poisoned");
	let pattern = db
		.get_tag_patterns(None)
		.ok()?
		.into_iter()
		.find(|p| p.id == id)?;
	let booru = db
		.get_booru_by_id(pattern.booru_id)
		.ok()
		.flatten()
		.map(|booru| booru.name)
		.unwrap_or_else(|| "unknown".to_string());
	Some(format!("{} / {booru}", pattern.name))
}

fn short_label(label: &str) -> String {
	let max = 70;
	if label.chars().count() <= max {
		label.to_string()
	} else {
		format!("{}...", label.chars().take(max - 3).collect::<String>())
	}
}

fn setting_modal(
	custom_id: impl Into<String>,
	title: impl Into<String>,
	key: &str,
	value: &str,
) -> CreateModal {
	CreateModal::new(custom_id, title).components(vec![
		CreateActionRow::InputText(
			CreateInputText::new(InputTextStyle::Short, "Setting Key", "setting_key")
				.required(true)
				.value(key),
		),
		CreateActionRow::InputText(
			CreateInputText::new(InputTextStyle::Paragraph, "Setting Value", "setting_value")
				.required(true)
				.value(value),
		),
	])
}

fn merged_booru_json(
	description: &str,
	urls_json: &str,
	paths_json: &str,
	options_json: &str,
) -> Result<String, String> {
	let mut output = serde_json::Map::new();
	merge_json_object(&mut output, urls_json, "API URLs JSON")?;
	merge_json_object(&mut output, paths_json, "JSON Paths JSON")?;
	merge_json_object(&mut output, options_json, "Options JSON")?;
	if !description.trim().is_empty() {
		output.insert(
			"description".to_string(),
			serde_json::Value::String(description.trim().to_string()),
		);
	}
	serde_json::to_string(&serde_json::Value::Object(output)).map_err(|err| err.to_string())
}

fn merge_json_object(
	output: &mut serde_json::Map<String, serde_json::Value>,
	input: &str,
	field: &str,
) -> Result<(), String> {
	let value = serde_json::from_str::<serde_json::Value>(input)
		.map_err(|err| format!("{field} must be valid JSON: {err}"))?;
	let serde_json::Value::Object(map) = value else {
		return Err(format!("{field} must be a JSON object"));
	};
	for (key, value) in map {
		output.insert(key, value);
	}
	Ok(())
}

async fn show_confirmation(
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	message: &str,
	action: &str,
) {
	let embed = CreateEmbed::new()
		.title("⚠️ Confirmation Required")
		.description(message)
		.color(0xED4245);

	let buttons = vec![
		CreateButton::new(format!("ay:{action}"))
			.label("Yes, proceed")
			.style(ButtonStyle::Danger),
		CreateButton::new("an")
			.label("Cancel")
			.style(ButtonStyle::Secondary),
	];

	let _ = interaction
		.create_response(
			&ctx.http,
			CreateInteractionResponse::UpdateMessage(
				CreateInteractionResponseMessage::new()
					.embed(embed)
					.components(vec![CreateActionRow::Buttons(buttons)]),
			),
		)
		.await;
}

async fn handle_confirmed_action(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	action_str: &str,
	is_admin: bool,
) {
	let parts = action_str.split(':').collect::<Vec<_>>();
	match parts.as_slice() {
		["booru", "delete", id] => {
			let id = id
				.parse::<i64>()
				.map_err(|_| "Invalid booru id".to_string());
			let result = id.and_then(|id| {
				let db = handler.db.lock().expect("db mutex poisoned");
				let booru = db
					.get_booru_by_id(id)
					.map_err(|err| err.to_string())?
					.ok_or_else(|| "Booru not found".to_string())?;
				db.delete_booru(&booru.name)
					.map_err(|err| err.to_string())?;
				Ok(format!(
					"Booru `{}` deleted. Reload to update commands.",
					booru.name
				))
			});
			respond_after_confirm(handler, interaction, ctx, result, "boorus", is_admin).await;
			return;
		}
		["booru", "toggle", id] => {
			let id = id
				.parse::<i64>()
				.map_err(|_| "Invalid booru id".to_string());
			let result = id.and_then(|id| {
				let db = handler.db.lock().expect("db mutex poisoned");
				let booru = db
					.get_booru_by_id(id)
					.map_err(|err| err.to_string())?
					.ok_or_else(|| "Booru not found".to_string())?;
				let enabled = !booru.enabled;
				db.set_booru_enabled(&booru.name, enabled)
					.map_err(|err| err.to_string())?;
				Ok(format!(
					"Booru `{}` {}. Reload to update commands.",
					booru.name,
					if enabled { "enabled" } else { "disabled" }
				))
			});
			respond_after_confirm(handler, interaction, ctx, result, "boorus", is_admin).await;
			return;
		}
		["pattern", "delete", category, id] => {
			let id = id
				.parse::<i64>()
				.map_err(|_| "Invalid pattern id".to_string());
			let result = id.and_then(|id| {
				let db = handler.db.lock().expect("db mutex poisoned");
				let pattern = db
					.get_tag_patterns(None)
					.map_err(|err| err.to_string())?
					.into_iter()
					.find(|pattern| pattern.id == id)
					.ok_or_else(|| "Pattern not found".to_string())?;
				let booru = db
					.get_booru_by_id(pattern.booru_id)
					.map_err(|err| err.to_string())?
					.ok_or_else(|| "Booru not found".to_string())?;
				db.delete_tag_pattern(&pattern.name, Some(&booru.name))
					.map_err(|err| err.to_string())?;
				Ok(format!(
					"Pattern `{}` removed from `{}`. Reload to update commands.",
					pattern.name, booru.name
				))
			});
			respond_after_confirm(handler, interaction, ctx, result, category, is_admin).await;
			return;
		}
		["channel", "remove", guild_id, channel_id] => {
			let result = parse_ids(guild_id, channel_id).and_then(|(guild_id, channel_id)| {
				let db = handler.db.lock().expect("db mutex poisoned");
				db.remove_channel(guild_id, channel_id)
					.map_err(|err| err.to_string())?;
				Ok(format!(
					"Channel `{channel_id}` removed from server `{guild_id}`."
				))
			});
			respond_after_confirm(handler, interaction, ctx, result, "channels", is_admin).await;
			return;
		}
		["settings", "delete", key] => {
			let result = {
				let db = handler.db.lock().expect("db mutex poisoned");
				db.delete_setting(key)
					.map(|()| format!("Setting `{key}` deleted."))
					.map_err(|err| err.to_string())
			};
			respond_after_confirm(handler, interaction, ctx, result, "settings", is_admin).await;
			return;
		}
		["server_tag", "remove", tag] => {
			let Some(guild_id) = interaction.guild_id.map(|g| g.get() as i64) else {
				let (embed, rows) = build_main_menu(
					handler,
					is_admin,
					interaction.guild_id.map(|g| g.get() as i64),
				);
				let _ = interaction
					.create_response(
						&ctx.http,
						CreateInteractionResponse::UpdateMessage(
							CreateInteractionResponseMessage::new()
								.content("Run this from inside a server channel.")
								.embed(embed)
								.components(rows),
						),
					)
					.await;
				return;
			};
			let result = {
				let db = handler.db.lock().expect("db mutex poisoned");
				db.remove_server_tag(guild_id, tag)
					.map(|()| format!("Server tag `{tag}` removed. Reload to update commands."))
					.map_err(|err| err.to_string())
			};
			respond_after_confirm(handler, interaction, ctx, result, "server_tags", is_admin).await;
			return;
		}
		_ => {}
	}

	let modal = match action_str {
		"booru:delete" => CreateModal::new("am:boorus:delete", "Delete Booru").components(vec![
			CreateActionRow::InputText(
				CreateInputText::new(InputTextStyle::Short, "Booru Name", "booru_name")
					.required(true),
			),
		]),
		"tag:remove" => CreateModal::new("am:tags:remove", "Remove Tag Pattern").components(vec![
			CreateActionRow::InputText(
				CreateInputText::new(InputTextStyle::Short, "Pattern Name", "pattern_name")
					.required(true),
			),
			CreateActionRow::InputText(
				CreateInputText::new(
					InputTextStyle::Short,
					"Booru Name (leave empty for all)",
					"booru_name",
				)
				.required(false),
			),
		]),
		"ch:remove" => CreateModal::new("am:channels:remove", "Remove Channel").components(vec![
			CreateActionRow::InputText(
				CreateInputText::new(
					InputTextStyle::Short,
					"Guild ID (leave empty for current)",
					"guild_id",
				)
				.required(false),
			),
			CreateActionRow::InputText(
				CreateInputText::new(InputTextStyle::Short, "Channel ID", "channel_id")
					.required(true),
			),
		]),
		_ => {
			let (embed, rows) = build_main_menu(
				handler,
				is_admin,
				interaction.guild_id.map(|g| g.get() as i64),
			);
			let _ = interaction
				.create_response(
					&ctx.http,
					CreateInteractionResponse::UpdateMessage(
						CreateInteractionResponseMessage::new()
							.embed(embed)
							.components(rows),
					),
				)
				.await;
			return;
		}
	};

	let _ = interaction
		.create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
		.await;
}

async fn respond_after_confirm(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
	result: Result<String, String>,
	category: &str,
	is_admin: bool,
) {
	let (embed, rows) = build_category_page(
		handler,
		category,
		is_admin,
		interaction.guild_id.map(|g| g.get() as i64),
	);
	let content = match result {
		Ok(msg) => msg,
		Err(err) => format!("Error: {err}"),
	};
	let _ = interaction
		.create_response(
			&ctx.http,
			CreateInteractionResponse::UpdateMessage(
				CreateInteractionResponseMessage::new()
					.content(content)
					.embed(embed)
					.components(rows),
			),
		)
		.await;
}

fn parse_ids(guild_id: &str, channel_id: &str) -> Result<(i64, i64), String> {
	let guild_id = guild_id
		.parse()
		.map_err(|_| "Invalid guild ID".to_string())?;
	let channel_id = channel_id
		.parse()
		.map_err(|_| "Invalid channel ID".to_string())?;
	Ok((guild_id, channel_id))
}

async fn process_modal_submission(
	handler: &Handler,
	custom_id: &str,
	components: &[serenity::all::ActionRow],
	interaction_guild_id: Option<i64>,
) -> Result<String, String> {
	let get_value = |id: &str| -> String {
		for row in components {
			for component in &row.components {
				if let serenity::all::ActionRowComponent::InputText(input) = component
					&& input.custom_id == id
				{
					return input.value.clone().unwrap_or_default();
				}
			}
		}
		String::new()
	};

	let rest = custom_id.strip_prefix("am:").unwrap_or(custom_id);
	let parts: Vec<&str> = rest.split(':').collect();
	let category = parts[0];
	let action = parts.get(1).copied().unwrap_or("");

	match (category, action) {
		("boorus", "add") => {
			let name = get_value("booru_name");
			let description = get_value("description");
			let urls_json = get_value("urls_json");
			let paths_json = get_value("paths_json");
			let options_json = get_value("options_json");
			if name.is_empty()
				|| urls_json.is_empty()
				|| paths_json.is_empty()
				|| options_json.is_empty()
			{
				return Err("Name, URLs, paths, and options are required.".to_string());
			}
			let json = merged_booru_json(&description, &urls_json, &paths_json, &options_json)?;
			let action_str = format!("add {name} {json}");
			let result = super::admin::booru::run_public(handler, &action_str).await;
			Ok(result)
		}
		("boorus", "edit") => {
			let name = if let Some(id) = parts.get(2) {
				booru_name(handler, id).ok_or_else(|| "Booru not found".to_string())?
			} else {
				get_value("booru_name")
			};
			let field = get_value("field_name");
			let value = get_value("field_value");
			let action_str = format!("edit {name} {field} {value}");
			let result = super::admin::booru::run_public(handler, &action_str).await;
			Ok(result)
		}
		("boorus", "delete") => {
			let name = get_value("booru_name");
			let action_str = format!("delete {name}");
			let result = super::admin::booru::run_public(handler, &action_str).await;
			Ok(result)
		}
		("boorus", "toggle") => {
			let name = get_value("booru_name");
			let db = handler.db.lock().expect("db mutex poisoned");
			let booru = db
				.get_booru_by_name(&name)
				.map_err(|e| e.to_string())?
				.ok_or_else(|| format!("Booru {name} not found"))?;
			let new_state = !booru.enabled;
			db.set_booru_enabled(&name, new_state)
				.map_err(|e| e.to_string())?;
			Ok(format!(
				"Booru {name} {}. Reload to apply.",
				if new_state { "enabled" } else { "disabled" }
			))
		}
		("boorus", "param") => {
			let name = if let Some(id) = parts.get(2) {
				booru_name(handler, id).ok_or_else(|| "Booru not found".to_string())?
			} else {
				get_value("booru_name")
			};
			let action = get_value("param_action");
			let key = get_value("param_key");
			let value = get_value("param_value");
			let action_str = match action.as_str() {
				"set" => format!("parameter set {name} {key} {value}"),
				"delete" => format!("parameter delete {name} {key}"),
				"list" => format!("parameter list {name}"),
				_ => return Err("Action must be set, delete, or list.".to_string()),
			};
			let result = super::admin::booru::run_public(handler, &action_str).await;
			Ok(result)
		}
		("settings", "set") => {
			let key = get_value("setting_key");
			let value = get_value("setting_value");
			if key.trim().is_empty() {
				return Err("Setting key is required.".to_string());
			}
			let db = handler.db.lock().expect("db mutex poisoned");
			db.set_setting(&key, &value)
				.map_err(|err| err.to_string())?;
			Ok(format!("Setting `{key}` set."))
		}
		("tags", "add") => {
			let pattern_name = get_value("pattern_name");
			let booru_name = get_value("booru_name");
			let included = get_value("included_tags");
			let excluded = get_value("excluded_tags");
			let action_str = if excluded.is_empty() {
				format!("{pattern_name} add {booru_name} {included}")
			} else {
				format!("{pattern_name} add {booru_name} {included} {excluded}")
			};
			let result = super::admin::tags::run_public(handler, &action_str).await;
			Ok(result)
		}
		("tags", "remove") => {
			let pattern_name = get_value("pattern_name");
			let booru_name = get_value("booru_name");
			let action_str = if booru_name.is_empty() {
				format!("{pattern_name} delete")
			} else {
				format!("{pattern_name} delete {booru_name}")
			};
			let result = super::admin::tags::run_public(handler, &action_str).await;
			Ok(result)
		}
		("server_tags", "add") => {
			let tag = get_value("tag_name");
			if tag.trim().is_empty() {
				return Err("Tag name is required.".to_string());
			}
			let Some(guild_id) = interaction_guild_id else {
				return Err("Run this from inside a server channel.".to_string());
			};
			let db = handler.db.lock().expect("db mutex poisoned");
			db.add_server_tag(guild_id, tag.trim())
				.map_err(|err| err.to_string())?;
			Ok(format!(
				"Server tag `{}` added. Reload to update commands.",
				tag.trim()
			))
		}
		("channels", "add") => {
			let guild_id = get_value("guild_id");
			let channel_id = get_value("channel_id");
			let db = handler.db.lock().expect("db mutex poisoned");
			let gid: i64 = if guild_id.is_empty() {
				return Err("Guild ID is required in modal.".to_string());
			} else {
				guild_id.parse().map_err(|_| "Invalid guild ID")?
			};
			let cid: i64 = channel_id.parse().map_err(|_| "Invalid channel ID")?;
			let cfg = crate::db::ChannelConfig {
				guild_id: gid,
				channel_id: cid,
				language: None,
				banned_tags: Vec::new(),
			};
			db.set_channel_config(&cfg).map_err(|e| e.to_string())?;
			Ok(format!("Channel {cid} added to server {gid}."))
		}
		("channels", "remove") => {
			let guild_id = get_value("guild_id");
			let channel_id = get_value("channel_id");
			let db = handler.db.lock().expect("db mutex poisoned");
			let gid: i64 = if guild_id.is_empty() {
				return Err("Guild ID is required.".to_string());
			} else {
				guild_id.parse().map_err(|_| "Invalid guild ID")?
			};
			let cid: i64 = channel_id.parse().map_err(|_| "Invalid channel ID")?;
			db.remove_channel(gid, cid).map_err(|e| e.to_string())?;
			Ok(format!("Channel {cid} removed from server {gid}."))
		}
		("channels", "set") => {
			let (gid, cid) = if parts.len() >= 4 {
				parse_ids(parts[2], parts[3])?
			} else {
				let guild_id = get_value("guild_id");
				let channel_id = get_value("channel_id");
				parse_ids(&guild_id, &channel_id)?
			};
			let language = get_value("language");
			let banned_tags = get_value("banned_tags");
			let db = handler.db.lock().expect("db mutex poisoned");
			let lang = if language.is_empty() {
				None
			} else {
				Some(language)
			};
			let banned: Vec<String> = if banned_tags.is_empty() {
				Vec::new()
			} else {
				banned_tags
					.split(',')
					.map(|s| s.trim().to_string())
					.filter(|s| !s.is_empty())
					.collect()
			};
			let cfg = crate::db::ChannelConfig {
				guild_id: gid,
				channel_id: cid,
				language: lang,
				banned_tags: banned,
			};
			db.set_channel_config(&cfg).map_err(|e| e.to_string())?;
			Ok(format!("Channel {cid} config updated."))
		}
		("channels", "pattern") => {
			let (gid, cid) = if parts.len() >= 4 {
				parse_ids(parts[2], parts[3])?
			} else {
				let guild_id = get_value("guild_id");
				let channel_id = get_value("channel_id");
				parse_ids(&guild_id, &channel_id)?
			};
			let action = get_value("pattern_action");
			let pattern_name = get_value("pattern_name");
			let db = handler.db.lock().expect("db mutex poisoned");
			match action.as_str() {
				"add" => {
					if pattern_name.is_empty() {
						return Err("Pattern name required.".to_string());
					}
					db.add_channel_pattern(gid, cid, &pattern_name)
						.map_err(|e| e.to_string())?;
					Ok(format!("Pattern {pattern_name} added to channel {cid}."))
				}
				"remove" => {
					if pattern_name.is_empty() {
						return Err("Pattern name required.".to_string());
					}
					db.remove_channel_pattern(gid, cid, &pattern_name)
						.map_err(|e| e.to_string())?;
					Ok(format!(
						"Pattern {pattern_name} removed from channel {cid}."
					))
				}
				"list" => {
					let patterns = db
						.get_channel_patterns(gid, cid)
						.map_err(|e| e.to_string())?;
					if patterns.is_empty() {
						Ok("No patterns assigned.".to_string())
					} else {
						Ok(patterns.join(", "))
					}
				}
				_ => Err("Action must be add, remove, or list.".to_string()),
			}
		}
		("validate", "validate") => {
			let guild_id = get_value("guild_id");
			let gid: i64 = guild_id.parse().map_err(|_| "Invalid guild ID")?;
			let db = handler.db.lock().expect("db mutex poisoned");
			db.set_validated(gid, true).map_err(|e| e.to_string())?;
			Ok(format!("Server {gid} validated."))
		}
		("validate", "unvalidate") => {
			let guild_id = get_value("guild_id");
			let gid: i64 = guild_id.parse().map_err(|_| "Invalid guild ID")?;
			let db = handler.db.lock().expect("db mutex poisoned");
			db.set_validated(gid, false).map_err(|e| e.to_string())?;
			Ok(format!("Server {gid} unvalidated."))
		}
		_ => Err(format!("Unknown modal action: {custom_id}")),
	}
}

async fn handle_reload_component(
	handler: &Handler,
	interaction: &ComponentInteraction,
	ctx: &serenity::all::Context,
) {
	let user_id = interaction.user.id.get() as i64;

	if user_id != handler.admin_user_id as i64 {
		let is_mod = {
			let db = handler.db.lock().expect("db mutex poisoned");
			let guild_id = interaction.guild_id.map(|g| g.get() as i64);
			db.is_moderator(user_id, guild_id).unwrap_or(false)
		};
		if !is_mod {
			let _ = interaction
				.create_response(
					&ctx.http,
					CreateInteractionResponse::Message(
						CreateInteractionResponseMessage::new()
							.content(handler.i18n.admin_only())
							.ephemeral(true),
					),
				)
				.await;
			return;
		}
	}

	if !handler.jobs.begin_reload() {
		let _ = interaction
			.create_response(
				&ctx.http,
				CreateInteractionResponse::Message(
					CreateInteractionResponseMessage::new()
						.content(handler.i18n.reload_toml_already_in_progress())
						.ephemeral(true),
				),
			)
			.await;
		return;
	}

	let _ = interaction
		.create_response(
			&ctx.http,
			CreateInteractionResponse::Message(
				CreateInteractionResponseMessage::new()
					.content(handler.i18n.reload_toml_waiting())
					.ephemeral(true),
			),
		)
		.await;

	let active_jobs = handler.jobs.active_count();
	if active_jobs != 0
		&& tokio::time::timeout(Duration::from_secs(30), handler.jobs.wait_idle())
			.await
			.is_err()
	{
		let _ = interaction
			.edit_response(
				&ctx.http,
				EditInteractionResponse::new().content(format!(
					"{active_jobs} active job(s) did not finish within 30 seconds. Reloading anyway."
				)),
			)
			.await;
	}

	handler.reload_requested.store(true, Ordering::Release);

	let _ = interaction
		.edit_response(
			&ctx.http,
			EditInteractionResponse::new().content(handler.i18n.reload_toml_finished()),
		)
		.await;

	let ctx_clone = ctx.clone();
	let shutdown_complete = Arc::clone(&handler.shutdown_complete);
	tokio::spawn(async move {
		tokio::time::sleep(Duration::from_millis(500)).await;
		ctx_clone.shard.shutdown_clean();
		shutdown_complete.notify_waiters();
	});
	handler.reload_notify.notify_waiters();
}

fn truncate(s: &str, max: usize) -> String {
	if s.len() <= max {
		s.to_string()
	} else {
		format!("{}...", &s[..max])
	}
}
