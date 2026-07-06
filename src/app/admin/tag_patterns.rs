use super::Handler;

pub(crate) async fn run(handler: &Handler, rest: &str) -> String {
	let (sub, args) =
		match super::split_required(rest, "usage: booru tag-pattern add|remove|list <booru> ...") {
			Ok(parts) => (parts.0.to_ascii_lowercase(), parts.1),
			Err(msg) => return msg,
		};

	match sub.as_str() {
		"add" => add(handler, args).await,
		"remove" | "delete" => remove(handler, args).await,
		"list" => list(handler, args).await,
		_ => "booru tag-pattern subcommands: add, remove, list".to_string(),
	}
}

async fn add(handler: &Handler, args: &str) -> String {
	let (booru_name, rest) = match super::split_required(
		args,
		"usage: booru tag-pattern add <booru> <name> <included_comma_sep> [excluded_comma_sep]",
	) {
		Ok(parts) => parts,
		Err(msg) => return msg,
	};
	let items: Vec<&str> = rest.splitn(3, ' ').collect();
	if items.len() < 2 {
		return "usage: booru tag-pattern add <booru> <name> <included_comma_sep> [excluded_comma_sep]".to_string();
	}
	let included = super::parse_csv(items[1]);
	let excluded = items
		.get(2)
		.map(|tags| super::parse_csv(tags))
		.unwrap_or_default();
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.add_tag_pattern(items[0], booru_name, &included, &excluded) {
		Ok(()) => format!("tag pattern '{}' added for {booru_name}", items[0]),
		Err(err) => format!("error: {err}"),
	}
}

async fn remove(handler: &Handler, args: &str) -> String {
	let (booru_name, pattern_name) =
		match super::split_required(args, "usage: booru tag-pattern remove <booru> <name>") {
			Ok(parts) => parts,
			Err(msg) => return msg,
		};
	if pattern_name.contains(' ') {
		return "usage: booru tag-pattern remove <booru> <name>".to_string();
	}
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.delete_tag_pattern(pattern_name, Some(booru_name)) {
		Ok(()) => format!("tag pattern '{pattern_name}' removed from {booru_name}"),
		Err(err) => format!("error: {err}"),
	}
}

async fn list(handler: &Handler, args: &str) -> String {
	let booru_name = match super::single_arg(args, "usage: booru tag-pattern list <booru>") {
		Ok(name) => name,
		Err(msg) => return msg,
	};
	let db = handler.db.lock().expect("db mutex poisoned");
	let booru = match db.get_booru_by_name(booru_name) {
		Ok(Some(booru)) => booru,
		Ok(None) => return format!("booru {booru_name} not found"),
		Err(err) => return format!("error: {err}"),
	};
	match db.get_tag_patterns(None) {
		Ok(patterns) => {
			let mut lines = Vec::new();
			for pattern in patterns
				.iter()
				.filter(|pattern| pattern.booru_id == booru.id)
			{
				let entries = db.get_pattern_entries(pattern.id).unwrap_or_default();
				lines.push(format!("{} ({} entries)", pattern.name, entries.len()));
				for entry in entries {
					let prefix = if entry.is_excluded { "-" } else { "+" };
					lines.push(format!("  {}{}", prefix, entry.tag));
				}
			}
			if lines.is_empty() {
				format!("no tag patterns for {booru_name}")
			} else {
				lines.join("\n")
			}
		}
		Err(err) => format!("error: {err}"),
	}
}
