use std::{
	collections::{BTreeMap, BTreeSet},
	env, fs,
	path::{Path, PathBuf},
};

const REQUIRED_KEYS: &[&str] = &[
	"administrate_action_description",
	"administrate_command_description",
	"administrate_help",
	"admin_only",
	"art_history_attachment_filename",
	"art_history_command_description",
	"art_history_error",
	"art_history_no_links",
	"art_history_option_description",
	"art_history_showing_all",
	"art_history_showing_count",
	"channel_not_allowed",
	"channel_patterns_empty",
	"could_not_find_image",
	"custom_command_description",
	"custom_command_no_tags",
	"custom_tag_option_description",
	"pattern_command_description",
	"reload_toml_already_in_progress",
	"reload_toml_command_description",
	"reload_toml_finished",
	"reload_toml_in_progress",
	"reload_toml_waiting",
	"required_tag_option_description",
	"server_not_validated",
];

const REQUIRED_PLACEHOLDERS: &[(&str, &[&str])] = &[
	("art_history_error", &["error"]),
	("art_history_showing_all", &["requested", "shown"]),
	("art_history_showing_count", &["shown"]),
	("could_not_find_image", &["error"]),
	("custom_command_description", &["booru"]),
	("pattern_command_description", &["pattern"]),
];

fn main() {
	println!("cargo:rerun-if-changed=locales");

	let locales_dir = Path::new("locales");
	let mut files = fs::read_dir(locales_dir)
		.unwrap_or_else(|err| panic!("failed to read locales directory: {err}"))
		.map(|entry| entry.unwrap().path())
		.filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
		.collect::<Vec<_>>();
	files.sort();

	if files.is_empty() {
		panic!("locales directory must contain at least one .toml file");
	}

	let required_keys = REQUIRED_KEYS.iter().copied().collect::<BTreeSet<_>>();
	let mut locales = Vec::new();

	for file in files {
		let language = language_from_file(&file);
		let raw = fs::read_to_string(&file)
			.unwrap_or_else(|err| panic!("failed to read locale file {}: {err}", file.display()));
		let values = toml::from_str::<BTreeMap<String, String>>(&raw)
			.unwrap_or_else(|err| panic!("failed to parse locale file {}: {err}", file.display()));

		let keys = values.keys().map(String::as_str).collect::<BTreeSet<_>>();
		let missing = required_keys.difference(&keys).copied().collect::<Vec<_>>();
		let extra = keys.difference(&required_keys).copied().collect::<Vec<_>>();

		if !missing.is_empty() {
			panic!(
				"locale file {} is missing keys: {}",
				file.display(),
				missing.join(", ")
			);
		}

		if !extra.is_empty() {
			panic!(
				"locale file {} has unknown keys: {}",
				file.display(),
				extra.join(", ")
			);
		}

		validate_placeholders(&file, &values);

		locales.push((language, values));
	}

	let mut sorted_keys: Vec<&str> = REQUIRED_KEYS.to_vec();
	sorted_keys.sort();

	let mut generated = String::new();
	generated.push_str("pub(crate) static TRANSLATIONS: &[(&str, &[(&str, &str)])] = &[\n");

	for (language, values) in locales {
		generated.push_str("    (");
		generated.push_str(&rust_string(&language));
		generated.push_str(", &[\n");
		for key in &sorted_keys {
			generated.push_str("        (");
			generated.push_str(&rust_string(key));
			generated.push_str(", ");
			generated.push_str(&rust_string(values.get(*key).unwrap()));
			generated.push_str("),\n");
		}
		generated.push_str("    ]),\n");
	}

	generated.push_str("];\n");

	let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
	fs::write(out_dir.join("i18n_generated.rs"), generated)
		.expect("failed to write generated i18n file");
}

fn language_from_file(file: &Path) -> String {
	let language = file
		.file_stem()
		.and_then(|stem| stem.to_str())
		.unwrap_or_else(|| panic!("locale file has invalid name: {}", file.display()))
		.trim()
		.to_ascii_lowercase();

	if language.is_empty()
		|| !language
			.chars()
			.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
	{
		panic!("locale file has invalid language name: {}", file.display());
	}

	language
}

fn validate_placeholders(file: &Path, values: &BTreeMap<String, String>) {
	for (key, expected) in REQUIRED_PLACEHOLDERS {
		let expected = expected.iter().copied().collect::<BTreeSet<_>>();
		let actual = placeholders(values.get(*key).unwrap()).collect::<BTreeSet<_>>();
		let missing = expected.difference(&actual).copied().collect::<Vec<_>>();
		let extra = actual.difference(&expected).copied().collect::<Vec<_>>();

		if !missing.is_empty() {
			panic!(
				"locale file {} key {} is missing placeholders: {}",
				file.display(),
				key,
				missing.join(", ")
			);
		}

		if !extra.is_empty() {
			panic!(
				"locale file {} key {} has unknown placeholders: {}",
				file.display(),
				key,
				extra.join(", ")
			);
		}
	}

	let expected_keys = REQUIRED_PLACEHOLDERS
		.iter()
		.map(|(key, _)| *key)
		.collect::<BTreeSet<_>>();
	for (key, value) in values {
		if expected_keys.contains(key.as_str()) {
			continue;
		}

		let placeholders = placeholders(value).collect::<Vec<_>>();
		if !placeholders.is_empty() {
			panic!(
				"locale file {} key {} does not support placeholders: {}",
				file.display(),
				key,
				placeholders.join(", ")
			);
		}
	}
}

fn placeholders(value: &str) -> impl Iterator<Item = &str> {
	value.match_indices('{').filter_map(|(start, _)| {
		let rest = &value[start + 1..];
		let end = rest.find('}')?;
		let placeholder = &rest[..end];

		(!placeholder.is_empty()
			&& placeholder
				.chars()
				.all(|ch| ch.is_ascii_lowercase() || ch == '_'))
		.then_some(placeholder)
	})
}

fn rust_string(value: &str) -> String {
	format!("{value:?}")
}
