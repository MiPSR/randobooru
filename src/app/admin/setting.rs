use super::Handler;

pub(crate) async fn run(handler: &Handler, rest: &str) -> String {
	let parts: Vec<&str> = rest.splitn(2, ' ').collect();
	let first = parts[0].trim().to_ascii_lowercase();
	let (key, args_str) = match first.as_str() {
		"list" | "" => (None, rest),
		_ => (Some(parts[0]), parts.get(1).copied().unwrap_or("")),
	};
	let (sub, args) = if key.is_some() {
		let aparts: Vec<&str> = args_str.splitn(2, ' ').collect();
		(
			aparts[0].trim().to_ascii_lowercase(),
			aparts.get(1).copied().unwrap_or(""),
		)
	} else {
		(first, parts.get(1).copied().unwrap_or(""))
	};

	match sub.as_str() {
		"get" => get(handler, key).await,
		"set" => set(handler, key, args).await,
		"list" => list(handler).await,
		"delete" => delete(handler, key).await,
		_ => "setting subcommands: get, set, list, delete".to_string(),
	}
}

async fn get(handler: &Handler, key: Option<&str>) -> String {
	let key = match key {
		Some(k) => k,
		None => return "usage: setting <key> get".to_string(),
	};
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.get_setting(key) {
		Ok(Some(v)) => format!("{key} = {v}"),
		Ok(None) => format!("{key} not set"),
		Err(err) => format!("error: {err}"),
	}
}

async fn set(handler: &Handler, key: Option<&str>, args: &str) -> String {
	let key = match key {
		Some(k) => k,
		None => return "usage: setting <key> set <value>".to_string(),
	};
	if args.is_empty() {
		return "usage: setting <key> set <value>".to_string();
	}
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.set_setting(key, args) {
		Ok(()) => format!("{key} set"),
		Err(err) => format!("error: {err}"),
	}
}

async fn list(handler: &Handler) -> String {
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.get_all_settings() {
		Ok(settings) => {
			if settings.is_empty() {
				"no settings".to_string()
			} else {
				settings
					.iter()
					.map(|(k, v)| format!("{k} = {v}"))
					.collect::<Vec<_>>()
					.join("\n")
			}
		}
		Err(err) => format!("error: {err}"),
	}
}

async fn delete(handler: &Handler, key: Option<&str>) -> String {
	let key = match key {
		Some(k) => k,
		None => return "usage: setting <key> delete".to_string(),
	};
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.delete_setting(key) {
		Ok(()) => format!("{key} deleted"),
		Err(err) => format!("error: {err}"),
	}
}
