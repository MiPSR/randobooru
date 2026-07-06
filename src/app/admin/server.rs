use super::Handler;

pub(crate) async fn run(
	handler: &Handler,
	rest: &str,
	default_guild_id: Option<i64>,
	default_channel_id: i64,
) -> String {
	let (guild_id, rest) = match super::try_parse_id(rest, default_guild_id) {
		Ok(v) => v,
		Err(e) => return e,
	};
	let parts: Vec<&str> = rest.splitn(2, ' ').collect();
	let sub = parts[0].trim().to_ascii_lowercase();
	let args = if parts.len() > 1 { parts[1].trim() } else { "" };

	match sub.as_str() {
		"validate" => validate(handler, guild_id).await,
		"unvalidate" => unvalidate(handler, guild_id).await,
		"list" => list_all(handler).await,
		"validated" => list_validated(handler).await,
		"channel" => channel(handler, guild_id, args, default_channel_id).await,
		"patterns" => patterns(handler, guild_id, args, default_channel_id).await,
		_ => "server subcommands: validate, unvalidate, list, validated, channel, patterns"
			.to_string(),
	}
}

async fn validate(handler: &Handler, guild_id: i64) -> String {
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.set_validated(guild_id, true) {
		Ok(()) => format!("server {guild_id} validated"),
		Err(err) => format!("error: {err}"),
	}
}

async fn unvalidate(handler: &Handler, guild_id: i64) -> String {
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.set_validated(guild_id, false) {
		Ok(()) => format!("server {guild_id} unvalidated"),
		Err(err) => format!("error: {err}"),
	}
}

async fn list_all(handler: &Handler) -> String {
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.get_all_servers() {
		Ok(servers) => {
			if servers.is_empty() {
				"no servers".to_string()
			} else {
				servers
					.iter()
					.map(|s| {
						format!(
							"{} {} [{}] channels={:?}",
							s.guild_id,
							s.name,
							if s.validated { "validated" } else { "pending" },
							s.interaction_channels
						)
					})
					.collect::<Vec<_>>()
					.join("\n")
			}
		}
		Err(err) => format!("error: {err}"),
	}
}

async fn list_validated(handler: &Handler) -> String {
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.get_validated_servers() {
		Ok(servers) => {
			if servers.is_empty() {
				"no validated servers".to_string()
			} else {
				servers
					.iter()
					.map(|s| format!("{} {}", s.guild_id, s.name))
					.collect::<Vec<_>>()
					.join("\n")
			}
		}
		Err(err) => format!("error: {err}"),
	}
}

async fn channel(handler: &Handler, guild_id: i64, args: &str, default_channel_id: i64) -> String {
	let (channel_id, ch_rest) = match super::try_parse_id(args, Some(default_channel_id)) {
		Ok(v) => v,
		Err(e) => return e,
	};
	let subparts: Vec<&str> = ch_rest.splitn(2, ' ').collect();
	let subaction = subparts[0].trim().to_ascii_lowercase();
	let subargs = subparts.get(1).copied().unwrap_or("");

	let db = handler.db.lock().expect("db mutex poisoned");
	match subaction.as_str() {
		"add" | "remove" => {
			let server = match db.get_server(guild_id) {
				Ok(Some(s)) => s,
				Ok(None) => return "server not found".to_string(),
				Err(err) => return format!("error: {err}"),
			};
			let mut channels = server.interaction_channels.clone();
			if subaction == "add" {
				if !channels.contains(&channel_id) {
					channels.push(channel_id);
				}
				match db.set_interaction_channels(guild_id, &channels) {
					Ok(()) => format!("channel {channel_id} added to server {guild_id}"),
					Err(err) => format!("error: {err}"),
				}
			} else {
				channels.retain(|&c| c != channel_id);
				match db.set_interaction_channels(guild_id, &channels) {
					Ok(()) => format!("channel {channel_id} removed from server {guild_id}"),
					Err(err) => format!("error: {err}"),
				}
			}
		}
		"set" => {
			let set_items: Vec<&str> = subargs.splitn(2, ' ').collect();
			let language = if !set_items.is_empty() && !set_items[0].is_empty() {
				Some(set_items[0].to_string())
			} else {
				None
			};
			let banned_tags: Vec<String> = if set_items.len() > 1 && !set_items[1].is_empty() {
				set_items[1]
					.split(',')
					.map(|s| s.trim().to_string())
					.filter(|s| !s.is_empty())
					.collect()
			} else {
				Vec::new()
			};
			let cfg = crate::db::ChannelConfig {
				guild_id,
				channel_id,
				language,
				banned_tags,
			};
			match db.set_channel_config(&cfg) {
				Ok(()) => format!(
					"channel {channel_id} config set: lang={}, banned={:?}",
					cfg.language.as_deref().unwrap_or("default"),
					cfg.banned_tags
				),
				Err(err) => format!("error: {err}"),
			}
		}
		"list" => match db.get_guild_channels(guild_id) {
			Ok(channels) => {
				if channels.is_empty() {
					"no channels configured".to_string()
				} else {
					channels
						.iter()
						.map(|c| {
							let patterns = db
								.get_channel_patterns(c.guild_id, c.channel_id)
								.unwrap_or_default();
							format!(
								"channel={} lang={} banned={:?} patterns={:?}",
								c.channel_id,
								c.language.as_deref().unwrap_or("default"),
								c.banned_tags,
								patterns
							)
						})
						.collect::<Vec<_>>()
						.join("\n")
				}
			}
			Err(err) => format!("error: {err}"),
		},
		_ => "usage: server <guild_id> channel <channel_id> add|remove|set|list ...".to_string(),
	}
}

async fn patterns(handler: &Handler, guild_id: i64, args: &str, default_channel_id: i64) -> String {
	let (channel_id, pat_rest) = match super::try_parse_id(args, Some(default_channel_id)) {
		Ok(v) => v,
		Err(e) => return e,
	};
	let pat_parts: Vec<&str> = pat_rest.splitn(2, ' ').collect();
	let action = pat_parts[0].trim().to_ascii_lowercase();
	let pattern_name = pat_parts.get(1).copied().unwrap_or("");

	let db = handler.db.lock().expect("db mutex poisoned");
	match action.as_str() {
		"add" => {
			if pattern_name.is_empty() {
				return "usage: server <guild_id> patterns <channel_id> add <name>".to_string();
			}
			match db.add_channel_pattern(guild_id, channel_id, pattern_name) {
				Ok(()) => format!("pattern {pattern_name} added to channel"),
				Err(err) => format!("error: {err}"),
			}
		}
		"remove" => {
			if pattern_name.is_empty() {
				return "usage: server <guild_id> patterns <channel_id> remove <name>".to_string();
			}
			match db.remove_channel_pattern(guild_id, channel_id, pattern_name) {
				Ok(()) => format!("pattern {pattern_name} removed from channel"),
				Err(err) => format!("error: {err}"),
			}
		}
		"list" => match db.get_channel_patterns(guild_id, channel_id) {
			Ok(patterns) => {
				if patterns.is_empty() {
					"no patterns assigned".to_string()
				} else {
					patterns.join("\n")
				}
			}
			Err(err) => format!("error: {err}"),
		},
		_ => "usage: server <guild_id> patterns <channel_id> add|remove|list [pattern_name]"
			.to_string(),
	}
}
