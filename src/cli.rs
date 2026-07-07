pub fn app_input(server: Option<i64>, channel: i64, user: i64, command: &str) {
	line(format!(
		"{} {} {} {} {}",
		timestamp(),
		normalize_option(server),
		normalize_i64(channel),
		normalize_i64(user),
		normalize(command)
	));
}

pub fn booru_random(step: &str, result: impl ToString) {
	line(format!(
		"Booru random step: {} {} {}",
		timestamp(),
		normalize(step),
		normalize(&result.to_string())
	));
}

pub fn request(url: &str) {
	line(format!("{} Request: {}", timestamp(), url));
}

pub fn final_kept(booru_source: Option<&str>, external_source: Option<&str>, picture: &str) {
	line(format!(
		"Final selection (kept): {} {} {} {}",
		timestamp(),
		normalize_optional_str(booru_source),
		normalize_optional_str(external_source),
		normalize(picture)
	));
}

pub fn final_kept_compressed(
	booru_source: Option<&str>,
	external_source: Option<&str>,
	picture: &str,
	compression_time: &str,
) {
	line(format!(
		"Final selection (kept + compressed): {} {} {} {} Compressed in {}",
		timestamp(),
		normalize_optional_str(booru_source),
		normalize_optional_str(external_source),
		normalize(picture),
		normalize(compression_time)
	));
}

pub fn final_retry(reason: &str, retry_count: usize) {
	line(format!(
		"Final selection (not kept, retrying): {} {} {}",
		timestamp(),
		normalize(reason),
		retry_count
	));
}

pub fn app_output(server: Option<i64>, channel: i64, output_type: &str) {
	line(format!(
		"{} {} {} {}",
		timestamp(),
		normalize_option(server),
		normalize_i64(channel),
		normalize(output_type)
	));
}

pub fn init_status(
	total_servers: usize,
	validated_servers: usize,
	channels: usize,
	commands: usize,
) {
	line(format!(
		"{} Found {} and {} and {} and {}.",
		timestamp(),
		plural(total_servers, "server"),
		plural(validated_servers, "validated server"),
		plural(channels, "configured channel"),
		plural(commands, "command"),
	));
}

pub fn checking_db() {
	line(format!("{} Checking the DB.", timestamp()));
}

pub fn db_cleaned(changes: usize) {
	line(format!(
		"{} DB cleaned {}.",
		timestamp(),
		plural(changes, "row")
	));
}

pub fn commands_cleaned() {
	line(format!("{} Commands cleaned.", timestamp()));
}

pub fn commands_pushed() {
	line(format!("{} Commands pushed.", timestamp()));
}

pub fn server_ready() {
	line(format!("{} Server ready.", timestamp()));
}

pub fn bot_loading() {
	line(format!("{} Bot loading.", timestamp()));
}

pub fn bot_reloading() {
	line(format!("{} Bot reloading.", timestamp()));
}

pub fn bot_loaded() {
	line(format!("{} Bot loaded.", timestamp()));
}

pub fn bot_reloaded() {
	line(format!("{} Bot reloaded.", timestamp()));
}

pub fn unloaded_everything() {
	line(format!("{} Unloaded everything.", timestamp()));
}

pub fn error(error: &str, error_type: &str) {
	line(format!(
		"{} {} {}",
		timestamp(),
		normalize(error),
		normalize(error_type)
	));
}

fn line(output: String) {
	eprintln!("{output}");
}

pub fn warn_missing_custom(booru: &str, key: &str) {
	line(format!(
		"{} WARNING: custom parameter {} not found for booru {}",
		timestamp(),
		key,
		booru
	));
}

fn timestamp() -> String {
	chrono::Utc::now()
		.format("%Y-%m-%dT%H:%M:%S%.3f")
		.to_string()
}

fn normalize_i64(value: i64) -> String {
	value.to_string()
}

fn normalize_option(value: Option<i64>) -> String {
	value.map(normalize_i64).unwrap_or_else(|| "-".to_string())
}

fn normalize_optional_str(value: Option<&str>) -> String {
	value.map(normalize).unwrap_or_else(|| "-".to_string())
}

fn normalize(value: &str) -> String {
	let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
	if normalized.is_empty() {
		"-".to_string()
	} else {
		normalized
	}
}

fn plural(n: usize, word: &str) -> String {
	if n <= 1 {
		format!("{n} {word}")
	} else {
		format!("{n} {word}s")
	}
}
