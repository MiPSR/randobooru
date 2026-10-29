use crate::db::BooruRow;

use super::servers::open_test_db;

fn test_booru(name: &str) -> BooruRow {
	BooruRow {
		id: 0,
		name: name.to_string(),
		enabled: true,
		embed_image: false,
		max_tags: 3,
		supports_character: true,
		page_size: 100,
		page_base: 1,
		tag_separator: " ".to_string(),
		encode_tag_separator: true,
		tag_spaces_as_plus: false,
		character_space_replacement: "_".to_string(),
		pace_ms: 0,
		description: String::new(),
		count_url: Some("https://test/count?tags={tags}".to_string()),
		count_path_json: "[]".to_string(),
		posts_url: "https://test/posts?tags={tags}&page={page}&limit={limit}".to_string(),
		posts_path_json: "[]".to_string(),
		file_url_path_json: r#"["file_url"]"#.to_string(),
		source_url_path_json: "[]".to_string(),
		detail_url: None,
		detail_id_path_json: "[]".to_string(),
		detail_file_url_path_json: "[]".to_string(),
		detail_source_url_path_json: "[]".to_string(),
		post_url: None,
		headers_json: "{}".to_string(),
		env_params_json: "[]".to_string(),
		fetch_options_json: "{}".to_string(),
	}
}

#[test]
fn settings_crud() {
	let db = open_test_db();
	assert!(db.get_setting("key").unwrap().is_none());

	db.set_setting("key", "val").unwrap();
	assert_eq!(db.get_setting("key").unwrap().unwrap(), "val");

	db.set_setting("key", "updated").unwrap();
	assert_eq!(db.get_setting("key").unwrap().unwrap(), "updated");

	db.delete_setting("key").unwrap();
	assert!(db.get_setting("key").unwrap().is_none());
}

#[test]
fn moderator_management() {
	let db = open_test_db();

	db.add_server(1, "test").unwrap();
	assert!(db.add_moderator(111, None, 999).is_err());
	db.add_moderator(111, Some(1), 999).unwrap();
	assert!(db.is_moderator(111, Some(1)).unwrap());
	assert!(!db.is_moderator(111, Some(2)).unwrap());
	assert!(!db.is_moderator(222, None).unwrap());

	db.add_moderator(333, Some(1), 999).unwrap();
	assert!(db.is_moderator(333, Some(1)).unwrap());

	db.remove_moderator(111, Some(1)).unwrap();
	assert!(!db.is_moderator(111, Some(1)).unwrap());
}

#[test]
fn booru_crud() {
	let db = open_test_db();

	let id = db.add_booru(&test_booru("demo")).unwrap();
	assert!(id > 0);

	let b = db.get_booru_by_name("demo").unwrap().unwrap();
	assert_eq!(b.name, "demo");
	assert!(b.enabled);

	db.set_booru_enabled("demo", false).unwrap();
	assert!(!db.get_booru_by_name("demo").unwrap().unwrap().enabled);

	assert_eq!(db.get_enabled_boorus().unwrap().len(), 0);

	db.set_booru_enabled("demo", true).unwrap();
	assert_eq!(db.get_enabled_boorus().unwrap().len(), 1);

	db.delete_booru("demo").unwrap();
	assert!(db.get_booru_by_name("demo").unwrap().is_none());
}

#[test]
fn tag_pattern_management() {
	let db = open_test_db();
	db.add_booru(&test_booru("demo")).unwrap();

	db.add_tag_pattern(
		"megane",
		"demo",
		&["glasses".to_string(), "1girl".to_string()],
		&["nudity".to_string()],
	)
	.unwrap();

	let patterns = db.get_tag_patterns(Some("megane")).unwrap();
	assert_eq!(patterns.len(), 1);

	let entries = db.get_pattern_entries(patterns[0].id).unwrap();
	assert_eq!(entries.len(), 3);
	assert!(entries.iter().any(|e| e.tag == "glasses" && !e.is_excluded));
	assert!(entries.iter().any(|e| e.tag == "nudity" && e.is_excluded));

	let ids = db.get_booru_ids_for_pattern("megane").unwrap();
	assert_eq!(ids.len(), 1);

	let names = db.get_unique_pattern_names().unwrap();
	assert!(names.contains(&"megane".to_string()));

	db.delete_tag_pattern("megane", Some("demo")).unwrap();
	assert!(db.get_tag_patterns(Some("megane")).unwrap().is_empty());
}

#[test]
fn booru_custom_parameter_management() {
	let db = open_test_db();
	db.add_booru(&test_booru("demo")).unwrap();

	db.set_booru_custom_parameter("demo", "session", "abc")
		.unwrap();
	db.set_booru_custom_parameter("demo", "anti_bot", "xyz")
		.unwrap();

	let parameters = db.get_booru_custom_parameters("demo").unwrap();
	assert_eq!(parameters.len(), 2);
	assert!(
		parameters
			.iter()
			.any(|p| p.key == "session" && p.value == "abc")
	);

	let all = db.get_all_booru_custom_parameters().unwrap();
	assert_eq!(all.len(), 2);

	db.delete_booru_custom_parameter("demo", "session").unwrap();
	let parameters = db.get_booru_custom_parameters("demo").unwrap();
	assert_eq!(parameters.len(), 1);
	assert_eq!(parameters[0].key, "anti_bot");
}

#[test]
fn art_history_per_channel() {
	let db = open_test_db();

	assert!(
		db.register_art("https://test/1", 100, Some(1), Some("demo"))
			.unwrap()
	);

	assert!(db.art_history_exists("https://test/1", 100).unwrap());
	assert!(!db.art_history_exists("https://test/1", 200).unwrap());

	assert!(
		db.register_art("https://test/1", 200, Some(1), Some("demo"))
			.unwrap()
	);
	assert!(db.art_history_exists("https://test/1", 200).unwrap());

	let recent_c100 = db.recent_art(100, 10).unwrap();
	assert_eq!(recent_c100.len(), 1);
	assert_eq!(recent_c100[0].source_link, "https://test/1");

	let recent_c200 = db.recent_art(200, 10).unwrap();
	assert_eq!(recent_c200.len(), 1);
}

#[test]
fn art_history_prune_per_channel() {
	let db = open_test_db();

	for i in 0..5 {
		db.register_art(
			&format!("https://test/c100/{i}"),
			100,
			Some(1),
			Some("demo"),
		)
		.unwrap();
	}

	let recent = db.recent_art(100, 10).unwrap();
	assert_eq!(recent.len(), 3);
	assert_eq!(recent[0].source_link, "https://test/c100/4");
	assert_eq!(recent[2].source_link, "https://test/c100/2");

	assert!(db.recent_art(200, 10).unwrap().is_empty());
}

#[test]
fn server_tag_whitelist_management() {
	let db = open_test_db();
	db.add_server(1, "test").unwrap();
	db.add_booru(&test_booru("demo")).unwrap();
	for tag in ["tag_a", "tag_b"] {
		db.add_tag_pattern(tag, "demo", &["solo".into()], &[])
			.unwrap();
	}

	assert!(db.get_server_tags(1).unwrap().is_empty());
	assert_eq!(db.count_server_tag_whitelist().unwrap(), 0);

	db.add_server_tag(1, "tag_a").unwrap();
	db.add_server_tag(1, "tag_b").unwrap();
	assert_eq!(db.count_server_tag_whitelist().unwrap(), 2);

	let tags = db.get_server_tags(1).unwrap();
	assert_eq!(tags, vec!["tag_a".to_string(), "tag_b".to_string()]);

	db.remove_server_tag(1, "tag_a").unwrap();
	assert_eq!(db.get_server_tags(1).unwrap(), vec!["tag_b".to_string()]);
}

#[test]
fn legacy_count_column_migrates_without_losing_patterns() {
	let db = open_test_db();
	let sql: String = db
		.connection()
		.query_row(
			"SELECT sql FROM sqlite_master WHERE name='boorus'",
			[],
			|r| r.get(0),
		)
		.unwrap();
	db.connection()
		.execute_batch("DROP TABLE boorus; PRAGMA user_version=0;")
		.unwrap();
	db.connection()
		.execute_batch(&sql.replace("count_url TEXT,", "count_url TEXT NOT NULL,"))
		.unwrap();
	db.add_booru(&test_booru("demo")).unwrap();
	db.add_tag_pattern("solo", "demo", &["solo".into()], &[])
		.unwrap();
	super::initialize_schema(db.connection()).unwrap();
	let mut row = db.get_booru_by_name("demo").unwrap().unwrap();
	row.count_url = None;
	db.update_booru(&row).unwrap();
	assert!(
		db.get_booru_by_name("demo")
			.unwrap()
			.unwrap()
			.count_url
			.is_none()
	);
	assert_eq!(db.get_tag_patterns(Some("solo")).unwrap().len(), 1);
	super::initialize_schema(db.connection()).unwrap();
}

#[test]
fn cleanup_handles_orphan_parent_and_child_rows_in_one_pass() {
	let db = open_test_db();
	db.connection()
		.execute_batch(
			"PRAGMA foreign_keys=OFF;
	INSERT INTO channels(guild_id,channel_id) VALUES(99,10);
	INSERT INTO server_tag_whitelist VALUES(99,'demo');
	INSERT INTO channel_patterns VALUES(99,10,'demo');
	PRAGMA foreign_keys=ON;",
		)
		.unwrap();
	assert_eq!(db.check_and_clean().unwrap(), 3);
	assert!(!db.has_channel(99, 10).unwrap());
}

#[test]
fn invalid_filter_data_aborts_cleanup_and_rolls_back() {
	let db = open_test_db();
	db.add_server(1, "test").unwrap();
	db.connection()
		.execute_batch(
			"PRAGMA foreign_keys=OFF;
	INSERT INTO tag_pattern_entries(pattern_id,tag) VALUES(99,'orphan');
	INSERT INTO channels(guild_id,channel_id,banned_tags) VALUES(1,10,'[\"blocked\",123]');
	PRAGMA foreign_keys=ON;",
		)
		.unwrap();
	assert!(db.check_and_clean().is_err());
	let remaining: i64 = db
		.connection()
		.query_row("SELECT COUNT(*) FROM tag_pattern_entries", [], |r| r.get(0))
		.unwrap();
	assert_eq!(remaining, 1);
	assert!(db.get_channel_config(1, 10).is_err());
}

#[test]
fn normalization_collisions_and_invalid_provider_values_are_rejected() {
	let db = open_test_db();
	let mut row = test_booru("demo");
	row.page_size = 0;
	assert!(db.add_booru(&row).is_err());
	row.page_size = 100;
	row.max_tags = 26;
	assert!(db.add_booru(&row).is_err());
	row.max_tags = 9;
	db.add_booru(&row).unwrap();
	db.add_tag_pattern("blue archive", "demo", &["blue_archive".into()], &[])
		.unwrap();
	assert!(
		db.add_tag_pattern("blue_archive", "demo", &["solo".into()], &[])
			.is_err()
	);
	assert!(
		db.add_tag_pattern("administrate", "demo", &["solo".into()], &[])
			.is_err()
	);
}

#[test]
fn persistent_dm_references_survive_reopen_and_expire_with_history() {
	use crate::{
		app::post_cache::{CachedPost, PostCache},
		booru::ImageResult,
	};
	use std::sync::{Arc, Mutex};
	let db = Arc::new(Mutex::new(open_test_db()));
	let cache = PostCache::new(Arc::clone(&db));
	let id = cache
		.store(
			CachedPost {
				image: ImageResult {
					image_url: "https://test/1.jpg".into(),
					post_url: Some("https://test/1".into()),
					upstream_source_url: None,
				},
				booru_name: "demo".into(),
				embed_image: false,
			},
			10,
		)
		.unwrap();
	{
		let db = db.lock().unwrap();
		db.register_art("https://test/1", 10, Some(1), Some("demo"))
			.unwrap();
		db.link_delivered_post(&id, "https://test/1", 10).unwrap();
	}
	drop(cache);
	let cache = PostCache::new(Arc::clone(&db));
	assert!(cache.get(&id, 10).unwrap().is_some());
	assert!(cache.get(&id, 11).unwrap().is_none());
	for n in 2..=4 {
		db.lock()
			.unwrap()
			.register_art(&format!("https://test/{n}"), 10, Some(1), Some("demo"))
			.unwrap();
	}
	assert!(cache.get(&id, 10).unwrap().is_none());
}

#[test]
fn channel_update_preserves_its_blacklist_and_normalizes_tags() {
	let db = open_test_db();
	db.add_server(1, "test").unwrap();
	db.add_booru(&test_booru("demo")).unwrap();
	db.add_tag_pattern("solo", "demo", &["solo".into()], &[])
		.unwrap();
	db.add_server_tag(1, "solo").unwrap();
	let mut config = super::ChannelConfig {
		guild_id: 1,
		channel_id: 10,
		language: None,
		banned_tags: vec![" Blocked ".into()],
	};
	db.set_channel_config(&config).unwrap();
	db.add_channel_pattern(1, 10, "solo").unwrap();
	config.language = Some("fr".into());
	db.set_channel_config(&config).unwrap();
	assert_eq!(db.get_channel_patterns(1, 10).unwrap(), vec!["solo"]);
	assert_eq!(
		db.get_channel_config(1, 10).unwrap().unwrap().banned_tags,
		vec!["blocked"]
	);
	db.remove_server_tag(1, "solo").unwrap();
	assert!(db.get_channel_patterns(1, 10).unwrap().is_empty());
}
