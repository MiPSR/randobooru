use std::fs;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct SecretConfig {
	pub discord_token: String,
	pub admin_user_id: u64,
	pub discord_application_id: u64,
}

pub(crate) fn parse() -> Result<SecretConfig> {
	let raw = fs::read_to_string("secrets.toml").context("failed to read secrets.toml")?;
	let secret: SecretConfig = toml::from_str(&raw).context("failed to parse secrets.toml")?;

	if secret.discord_token.trim().is_empty() {
		bail!("secrets.toml discord_token is required");
	}

	if secret.discord_application_id == 0 {
		bail!("secrets.toml discord_application_id must be greater than zero");
	}

	if secret.admin_user_id == 0 {
		bail!("secrets.toml admin_user_id must be greater than zero");
	}

	Ok(secret)
}
