use crate::{db::Database, i18n::I18n};
use anyhow::{Result, ensure};

pub const DEFAULTS: &[(&str, &str)] = &[
	("app_lang", "en"),
	("api_rate_pace_ms", "0"),
	("discord_api_pace_ms", "0"),
	("booru_fetch_retry_limit", "3"),
	("booru_source_link_history_limit", "1000"),
	("booru_tag_blacklist", ""),
	("command_sync_interval_secs", "86400"),
	("http_timeout_secs", "30"),
	("max_concurrent_jobs", "4"),
];

pub fn validate(key: &str, value: &str) -> Result<()> {
	ensure!(
		!key.trim().is_empty() && key.chars().count() <= 100 && value.chars().count() <= 4000,
		"setting key/value is empty or too long"
	);
	let bounds = match key {
		"app_lang" => {
			I18n::load(value)?;
			return Ok(());
		}
		"api_rate_pace_ms" | "discord_api_pace_ms" => Some((0, 60_000)),
		"booru_fetch_retry_limit" => Some((0, 20)),
		"booru_source_link_history_limit" => Some((1, 100_000)),
		"command_sync_interval_secs" => Some((60, 2_678_400)),
		"http_timeout_secs" => Some((1, 120)),
		"max_concurrent_jobs" => Some((1, 32)),
		_ => None,
	};
	if let Some((min, max)) = bounds {
		let number: u64 = value
			.parse()
			.map_err(|_| anyhow::anyhow!("{key} must be a whole number"))?;
		ensure!((min..=max).contains(&number), "{key} must be {min}..{max}");
	}
	Ok(())
}

pub fn get(db: &Database, key: &str) -> Result<String> {
	let value = db.get_setting(key)?.unwrap_or_else(|| {
		DEFAULTS
			.iter()
			.find(|(k, _)| *k == key)
			.map(|(_, v)| v.to_string())
			.unwrap_or_default()
	});
	validate(key, &value)?;
	Ok(value)
}

pub fn number(db: &Database, key: &str) -> Result<u64> {
	Ok(get(db, key)?.parse()?)
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn validates_defaults_and_bounds() {
		for (key, value) in DEFAULTS {
			validate(key, value).unwrap();
		}
		for (key, value) in [
			("app_lang", "bad"),
			("booru_fetch_retry_limit", "-1"),
			("http_timeout_secs", "0"),
			("max_concurrent_jobs", "1000"),
		] {
			assert!(validate(key, value).is_err());
		}
	}
}
