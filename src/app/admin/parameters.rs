use super::Handler;

pub(crate) async fn run(handler: &Handler, rest: &str) -> String {
	let (sub, args) =
		match super::split_required(rest, "usage: booru parameter set|delete|list <booru> ...") {
			Ok(parts) => (parts.0.to_ascii_lowercase(), parts.1),
			Err(msg) => return msg,
		};

	match sub.as_str() {
		"set" => set(handler, args).await,
		"delete" | "remove" => delete(handler, args).await,
		"list" => list(handler, args).await,
		_ => "booru parameter subcommands: set, delete, list".to_string(),
	}
}

async fn set(handler: &Handler, args: &str) -> String {
	let (booru_name, rest) =
		match super::split_required(args, "usage: booru parameter set <booru> <key> <value>") {
			Ok(parts) => parts,
			Err(msg) => return msg,
		};
	let (key, value) =
		match super::split_required(rest, "usage: booru parameter set <booru> <key> <value>") {
			Ok(parts) => parts,
			Err(msg) => return msg,
		};
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.set_booru_custom_parameter(booru_name, key, value) {
		Ok(()) => {
			format!("booru parameter {key} set for {booru_name}. reload bot to apply.")
		}
		Err(err) => format!("error: {err}"),
	}
}

async fn delete(handler: &Handler, args: &str) -> String {
	let (booru_name, key) =
		match super::split_required(args, "usage: booru parameter delete <booru> <key>") {
			Ok(parts) => parts,
			Err(msg) => return msg,
		};
	if key.contains(' ') {
		return "usage: booru parameter delete <booru> <key>".to_string();
	}
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.delete_booru_custom_parameter(booru_name, key) {
		Ok(()) => {
			format!("booru parameter {key} deleted for {booru_name}. reload bot to apply.")
		}
		Err(err) => format!("error: {err}"),
	}
}

async fn list(handler: &Handler, args: &str) -> String {
	let booru_name = match super::single_arg(args, "usage: booru parameter list <booru>") {
		Ok(name) => name,
		Err(msg) => return msg,
	};
	let db = handler.db.lock().expect("db mutex poisoned");
	match db.get_booru_custom_parameters(booru_name) {
		Ok(parameters) => {
			if parameters.is_empty() {
				format!("no booru parameters for {booru_name}")
			} else {
				parameters
					.iter()
					.map(|parameter| format!("{} = {}", parameter.key, parameter.value))
					.collect::<Vec<_>>()
					.join("\n")
			}
		}
		Err(err) => format!("error: {err}"),
	}
}
