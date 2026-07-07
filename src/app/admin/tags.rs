use super::Handler;

#[allow(dead_code)]
pub(crate) async fn run(handler: &Handler, rest: &str) -> String {
	run_public(handler, rest).await
}

pub(crate) async fn run_public(handler: &Handler, rest: &str) -> String {
	let parts: Vec<&str> = rest.splitn(2, ' ').collect();
	let first = parts[0].trim().to_ascii_lowercase();
	let (name, args_str) = match first.as_str() {
		"list" | "" => (None, rest),
		_ => (Some(parts[0]), parts.get(1).copied().unwrap_or("")),
	};
	let (sub, args) = if name.is_some() {
		let aparts: Vec<&str> = args_str.splitn(2, ' ').collect();
		(
			aparts[0].trim().to_ascii_lowercase(),
			aparts.get(1).copied().unwrap_or(""),
		)
	} else {
		(first, parts.get(1).copied().unwrap_or(""))
	};

	match sub.as_str() {
		"add" => add(handler, name, args).await,
		"delete" => delete(handler, name, args).await,
		"list" => list(handler).await,
		_ => "tags subcommands: add, delete, list".to_string(),
	}
}

async fn add(handler: &Handler, name: Option<&str>, args: &str) -> String {
	let name = match name {
		Some(n) => n,
		None => {
			return "usage: tags <name> add <booru> <included_comma_sep> [excluded_comma_sep]"
				.to_string();
		}
	};
	let items: Vec<&str> = args.splitn(3, ' ').collect();
	if items.len() < 2 {
		return "usage: tags <name> add <booru> <included_comma_sep> [excluded_comma_sep]"
			.to_string();
	}
	let booru_name = items[0];
	let included: Vec<String> = items[1]
		.split(',')
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.collect();
	let excluded = items
		.get(2)
		.map(|tags| super::parse_csv(tags))
		.unwrap_or_default();
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.add_tag_pattern(name, booru_name, &included, &excluded) {
		Ok(()) => format!("tag pattern '{name}' added for {booru_name}"),
		Err(err) => format!("error: {err}"),
	}
}

async fn delete(handler: &Handler, name: Option<&str>, args: &str) -> String {
	let name = match name {
		Some(n) => n,
		None => return "usage: tags <name> delete [booru]".to_string(),
	};
	let booru = if args.is_empty() { None } else { Some(args) };
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.delete_tag_pattern(name, booru) {
		Ok(()) => format!("tag pattern '{name}' deleted"),
		Err(err) => format!("error: {err}"),
	}
}

async fn list(handler: &Handler) -> String {
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.get_tag_patterns(None) {
		Ok(patterns) => {
			if patterns.is_empty() {
				"no tag patterns".to_string()
			} else {
				let mut lines = Vec::new();
				for pattern in &patterns {
					let booru_name = db
						.get_booru_by_id(pattern.booru_id)
						.ok()
						.flatten()
						.map(|b| b.name)
						.unwrap_or_else(|| format!("<missing:{}>", pattern.booru_id));
					let entries = db.get_pattern_entries(pattern.id).unwrap_or_default();
					lines.push(format!(
						"{} -> {} ({} entries)",
						pattern.name,
						booru_name,
						entries.len()
					));
					for entry in &entries {
						let prefix = if entry.is_excluded { "-" } else { "+" };
						lines.push(format!(
							"  [{}] (pattern:{}) {}{}",
							entry.id, entry.pattern_id, prefix, entry.tag
						));
					}
				}
				lines.join("\n")
			}
		}
		Err(err) => format!("error: {err}"),
	}
}
