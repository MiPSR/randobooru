use super::Handler;

pub(crate) async fn run(handler: &Handler, rest: &str) -> String {
	let parts: Vec<&str> = rest.splitn(2, ' ').collect();
	let first = parts[0].trim().to_ascii_lowercase();
	let (user_id, args_str) = match first.as_str() {
		"list" | "" => (None, rest),
		_ => match parts[0].parse::<i64>() {
			Ok(id) => (Some(id), parts.get(1).copied().unwrap_or("")),
			Err(_) => return format!("invalid user_id '{}'", parts[0]),
		},
	};
	let (sub, args) = if user_id.is_some() {
		let aparts: Vec<&str> = args_str.splitn(2, ' ').collect();
		(
			aparts[0].trim().to_ascii_lowercase(),
			aparts.get(1).copied().unwrap_or(""),
		)
	} else {
		(first, parts.get(1).copied().unwrap_or(""))
	};

	match sub.as_str() {
		"add" => add(handler, user_id, args).await,
		"remove" => remove(handler, user_id, args).await,
		"list" => list(handler).await,
		_ => "moderator subcommands: add, remove, list".to_string(),
	}
}

async fn add(handler: &Handler, user_id: Option<i64>, args: &str) -> String {
	let user_id = match user_id {
		Some(id) => id,
		None => return "usage: moderator <user_id> add [guild_id]".to_string(),
	};
	let guild_id: Option<i64> = if args.is_empty() {
		None
	} else {
		match args.parse() {
			Ok(v) => Some(v),
			Err(_) => return "invalid guild_id".to_string(),
		}
	};
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.add_moderator(user_id, guild_id, handler.admin_user_id as i64) {
		Ok(()) => format!("moderator {user_id} added"),
		Err(err) => format!("error: {err}"),
	}
}

async fn remove(handler: &Handler, user_id: Option<i64>, args: &str) -> String {
	let user_id = match user_id {
		Some(id) => id,
		None => return "usage: moderator <user_id> remove [guild_id]".to_string(),
	};
	let guild_id: Option<i64> = if args.is_empty() {
		None
	} else {
		match args.parse() {
			Ok(v) => Some(v),
			Err(_) => return "invalid guild_id".to_string(),
		}
	};
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.remove_moderator(user_id, guild_id) {
		Ok(()) => format!("moderator {user_id} removed"),
		Err(err) => format!("error: {err}"),
	}
}

async fn list(handler: &Handler) -> String {
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.list_moderators() {
		Ok(mods) => {
			if mods.is_empty() {
				"no moderators".to_string()
			} else {
				mods.iter()
					.map(|(uid, gid)| format!("{uid} {:?}", gid))
					.collect::<Vec<_>>()
					.join("\n")
			}
		}
		Err(err) => format!("error: {err}"),
	}
}
