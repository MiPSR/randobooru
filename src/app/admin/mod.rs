use anyhow::{bail, Context, Result};
use serenity::all::CommandInteraction;

mod booru;
mod moderator;
mod parameters;
mod server;
mod setting;
mod tag_patterns;
mod tags;

use super::Handler;

pub(crate) async fn execute_admin_action(
	handler: &Handler,
	action: &str,
	command: &CommandInteraction,
) -> String {
	let parts: Vec<&str> = action.splitn(2, ' ').collect();
	let cmd = parts[0].trim().to_ascii_lowercase();
	let rest = if parts.len() > 1 { parts[1].trim() } else { "" };

	let current_guild = command.guild_id.map(|g| g.get() as i64);
	let current_channel = command.channel_id.get() as i64;

	match cmd.as_str() {
		"help" => handler.i18n.administrate_help().to_string(),
		"booru" => booru::run(handler, rest).await,
		"tags" => tags::run(handler, rest).await,
		"moderator" => moderator::run(handler, rest).await,
		"server" => server::run(handler, rest, current_guild, current_channel).await,
		"setting" => setting::run(handler, rest).await,
		"channel" => match current_guild {
			Some(gid) => {
				Box::pin(server::run(
					handler,
					&format!("{gid} channel {rest}"),
					current_guild,
					current_channel,
				))
				.await
			}
			None => "no server context; use 'server <guild_id> channel ...'".to_string(),
		},
		"patterns" => {
			if rest.is_empty() {
				return "usage: patterns add|remove|list [name]".to_string();
			}
			let add_rest = format!(
				"{} patterns {} {}",
				current_guild.unwrap_or(0),
				current_channel,
				rest
			);
			Box::pin(server::run(
				handler,
				&add_rest,
				current_guild,
				current_channel,
			))
			.await
		}
		_ => format!("unknown administrate command: {cmd}\nuse 'help'"),
	}
}

pub(super) fn try_parse_id(
	s: &str,
	default: Option<i64>,
) -> std::result::Result<(i64, &str), String> {
	if s.is_empty() {
		if let Some(d) = default {
			return Ok((d, ""));
		}
		return Err("no id provided".to_string());
	}
	let parts: Vec<&str> = s.splitn(2, ' ').collect();
	if let Ok(id) = parts[0].parse::<i64>() {
		Ok((id, parts.get(1).copied().unwrap_or("")))
	} else if let Some(d) = default {
		Ok((d, s))
	} else {
		Err(format!("expected a numeric id, got '{}'", parts[0]))
	}
}

pub(super) fn split_required<'a>(
	input: &'a str,
	usage: &str,
) -> std::result::Result<(&'a str, &'a str), String> {
	let parts: Vec<&str> = input.trim().splitn(2, ' ').collect();
	if parts.len() < 2 || parts[0].trim().is_empty() || parts[1].trim().is_empty() {
		return Err(usage.to_string());
	}
	Ok((parts[0].trim(), parts[1].trim()))
}

pub(super) fn single_arg<'a>(input: &'a str, usage: &str) -> std::result::Result<&'a str, String> {
	let value = input.trim();
	if value.is_empty() || value.contains(' ') {
		return Err(usage.to_string());
	}
	Ok(value)
}

pub(super) fn parse_csv(input: &str) -> Vec<String> {
	input
		.split(',')
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.collect()
}

pub(super) fn parse_bool(value: &str) -> Result<bool> {
	match value.trim().to_ascii_lowercase().as_str() {
		"true" | "1" | "yes" | "on" => Ok(true),
		"false" | "0" | "no" | "off" => Ok(false),
		_ => bail!("expected boolean value"),
	}
}

pub(super) fn non_empty<'a>(value: &'a str, field: &str) -> Result<&'a str> {
	if value.is_empty() {
		bail!("{field} cannot be empty");
	}
	Ok(value)
}

pub(super) fn optional_string(value: &str) -> Option<String> {
	if value.eq_ignore_ascii_case("null") || value.is_empty() {
		None
	} else {
		Some(value.to_string())
	}
}

pub(super) fn normalized_json(value: &str) -> Result<String> {
	let parsed: serde_json::Value = serde_json::from_str(value).context("expected valid json")?;
	Ok(serde_json::to_string(&parsed)?)
}
