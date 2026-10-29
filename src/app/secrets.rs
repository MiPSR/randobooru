use std::{env, fs};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct SecretConfig {
	pub discord_token: String,
	pub admin_user_id: u64,
	pub discord_application_id: u64,
}

pub(crate) fn parse() -> Result<SecretConfig> {
	let path = env::var("RANDOBOORU_SECRETS_FILE").unwrap_or_else(|_| "secrets.toml".into());
	parse_path(std::path::Path::new(&path))
}

fn parse_path(path: &std::path::Path) -> Result<SecretConfig> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		let mode = fs::metadata(path)
			.with_context(|| format!("cannot access runtime secrets file {}", path.display()))?
			.permissions()
			.mode();
		if mode & 0o077 != 0 {
			bail!(
				"runtime secrets file {} must not be accessible by group or others (use chmod 600)",
				path.display()
			);
		}
	}
	let raw = fs::read_to_string(path)
		.with_context(|| format!("failed to read runtime secrets file {}", path.display()))?;
	let secret: SecretConfig = toml::from_str(&raw).map_err(|_| {
		anyhow::anyhow!("invalid secrets TOML: check the required keys and value types")
	})?;

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

#[cfg(all(test, unix))]
mod tests {
	use super::*;
	use std::os::unix::fs::PermissionsExt;
	#[test]
	fn secret_errors_never_print_credential_lines_and_permissions_are_checked() {
		let path = std::env::temp_dir().join(format!(
			"randobooru-secrets-test-{}",
			crate::app::post_cache::unique_id()
		));
		let mut options = fs::OpenOptions::new();
		use std::{io::Write, os::unix::fs::OpenOptionsExt};
		let mut file = options
			.write(true)
			.create_new(true)
			.mode(0o600)
			.open(&path)
			.unwrap();
		write!(
			file,
			"discord_token = \"audit-canary-secret\"\nadmin_user_id = invalid"
		)
		.unwrap();
		let error = parse_path(&path).err().unwrap().to_string();
		assert!(!error.contains("audit-canary-secret"));
		fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
		assert!(
			parse_path(&path)
				.err()
				.unwrap()
				.to_string()
				.contains("chmod 600")
		);
		fs::remove_file(path).unwrap();
	}
}
