use anyhow::{bail, Result};

include!(concat!(env!("OUT_DIR"), "/i18n_generated.rs"));

#[derive(Clone)]
pub struct I18n {
	translations: &'static [(&'static str, &'static str)],
}

impl I18n {
	pub fn load(language: &str) -> Result<Self> {
		let language = normalize_language(language);
		let Some((_, translations)) = TRANSLATIONS.iter().find(|(name, _)| *name == language)
		else {
			bail!("unsupported app_lang {language}");
		};

		Ok(Self { translations })
	}

	pub fn available_languages() -> Vec<&'static str> {
		TRANSLATIONS.iter().map(|(name, _)| *name).collect()
	}

	pub fn could_not_find_image(&self, error: &str) -> String {
		self.render("could_not_find_image", &[("error", error)])
	}

	pub fn reload_toml_already_in_progress(&self) -> &'static str {
		self.text("reload_toml_already_in_progress")
	}

	pub fn reload_toml_waiting(&self) -> &'static str {
		self.text("reload_toml_waiting")
	}

	pub fn reload_toml_finished(&self) -> &'static str {
		self.text("reload_toml_finished")
	}

	pub fn reload_toml_in_progress(&self) -> &'static str {
		self.text("reload_toml_in_progress")
	}

	pub fn art_history_error(&self, error: &str) -> String {
		self.render("art_history_error", &[("error", error)])
	}

	pub fn art_history_no_links(&self) -> &'static str {
		self.text("art_history_no_links")
	}

	pub fn art_history_showing_all(&self, requested: usize, shown: usize) -> String {
		let requested = requested.to_string();
		let shown = shown.to_string();
		self.render(
			"art_history_showing_all",
			&[("requested", &requested), ("shown", &shown)],
		)
	}

	pub fn art_history_showing_count(&self, shown: usize) -> String {
		let shown = shown.to_string();
		self.render("art_history_showing_count", &[("shown", &shown)])
	}

	pub fn art_history_attachment_filename(&self) -> &'static str {
		self.text("art_history_attachment_filename")
	}

	pub fn custom_command_no_tags(&self) -> &'static str {
		self.text("custom_command_no_tags")
	}

	pub fn custom_command_description(&self, booru: &str) -> String {
		self.render("custom_command_description", &[("booru", booru)])
	}

	pub fn required_tag_option_description(&self) -> &'static str {
		self.text("required_tag_option_description")
	}

	pub fn custom_tag_option_description(&self) -> &'static str {
		self.text("custom_tag_option_description")
	}

	pub fn art_history_command_description(&self) -> &'static str {
		self.text("art_history_command_description")
	}

	pub fn art_history_option_description(&self) -> &'static str {
		self.text("art_history_option_description")
	}

	pub fn reload_toml_command_description(&self) -> &'static str {
		self.text("reload_toml_command_description")
	}

	pub fn server_not_validated(&self) -> &'static str {
		self.text("server_not_validated")
	}

	pub fn channel_not_allowed(&self) -> &'static str {
		self.text("channel_not_allowed")
	}

	pub fn admin_only(&self) -> &'static str {
		self.text("admin_only")
	}

	pub fn administrate_help(&self) -> &'static str {
		self.text("administrate_help")
	}

	pub fn administrate_command_description(&self) -> &'static str {
		self.text("administrate_command_description")
	}

	pub fn administrate_action_description(&self) -> &'static str {
		self.text("administrate_action_description")
	}

	pub fn pattern_command_description(&self, pattern: &str) -> String {
		self.render("pattern_command_description", &[("pattern", pattern)])
	}

	fn text(&self, key: &str) -> &'static str {
		self.translations
			.binary_search_by(|(candidate, _)| candidate.cmp(&key))
			.map(|index| self.translations[index].1)
			.expect("locale key should have been validated at compile time")
	}

	fn render(&self, key: &str, values: &[(&str, &str)]) -> String {
		let mut output = self.text(key).to_string();

		for (name, value) in values {
			output = output.replace(&format!("{{{name}}}"), value);
		}

		output
	}
}

fn normalize_language(language: &str) -> String {
	language.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn loads_language_from_locale_file() {
		let i18n = I18n::load("fr").expect("fr locale should exist");

		assert_eq!(
			i18n.could_not_find_image("test"),
			"Impossible de trouver une image : test"
		);
	}

	#[test]
	fn rejects_unknown_language() {
		assert!(I18n::load("missing-language").is_err());
	}
}
