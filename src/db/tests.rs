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
		count_url: "https://test/count?tags={tags}".to_string(),
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

	db.add_moderator(111, None, 999).unwrap();
	assert!(db.is_moderator(111, None).unwrap());
	assert!(!db.is_moderator(222, None).unwrap());

	db.add_moderator(333, Some(1), 999).unwrap();
	assert!(db.is_moderator(333, Some(1)).unwrap());

	db.remove_moderator(111, None).unwrap();
	assert!(!db.is_moderator(111, None).unwrap());
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
	assert!(parameters
		.iter()
		.any(|p| p.key == "session" && p.value == "abc"));

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

	assert!(db
		.register_art("https://test/1", 100, Some(1), Some("demo"))
		.unwrap());

	assert!(db.art_history_exists("https://test/1", 100).unwrap());
	assert!(!db.art_history_exists("https://test/1", 200).unwrap());

	assert!(db
		.register_art("https://test/1", 200, Some(1), Some("demo"))
		.unwrap());
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
