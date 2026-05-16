//! Integration tests for `SessionStore` persistence, listing, and deletion.

use openlibertas_core::domain::{Message, Role};
use openlibertas_core::store::SessionStore;

fn user_msg(content: &str) -> Message {
    Message {
        role: Role::User,
        content: content.to_string(),
        tool_calls: None,
        tool_call_id: None,
        timestamp: None,
        reasoning_content: None,
        is_prompt: false,
    }
}

fn assistant_msg(content: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: content.to_string(),
        tool_calls: None,
        tool_call_id: None,
        timestamp: None,
        reasoning_content: None,
        is_prompt: false,
    }
}

#[test]
fn store_creates_directory_on_new() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("nested").join("store");
    assert!(!sub.exists());
    let _store = SessionStore::new(sub.clone()).unwrap();
    assert!(sub.exists());
}

#[test]
fn store_save_and_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();

    let messages = vec![
        user_msg("hello"),
        assistant_msg("hi there"),
    ];

    store.save("session-1", Some("model-x"), &messages).unwrap();
    let loaded = store.load("session-1").unwrap();

    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].content, "hello");
    assert_eq!(loaded[1].content, "hi there");
}

#[test]
fn store_list_returns_sorted_sessions() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();

    store.save("a", None, &[user_msg("a")]).unwrap();
    store.save("b", None, &[user_msg("b")]).unwrap();
    store.save("c", None, &[user_msg("c")]).unwrap();

    let sessions = store.list_with_meta().unwrap();
    assert_eq!(sessions.len(), 3);
    // Most-recently-updated first.
    assert_eq!(sessions[0].id, "c");
    assert_eq!(sessions[1].id, "b");
    assert_eq!(sessions[2].id, "a");
}

#[test]
fn store_list_with_empty_store() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();
    let sessions = store.list_with_meta().unwrap();
    assert!(sessions.is_empty());
}

#[test]
fn store_delete_removes_session() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();

    store.save("to-delete", None, &[user_msg("x")]).unwrap();
    assert!(store.session_path("to-delete").exists());

    store.delete("to-delete").unwrap();
    assert!(!store.session_path("to-delete").exists());
}

#[test]
fn store_delete_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();
    // Deleting a non-existent session should not error.
    store.delete("never-saved").unwrap();
}

#[test]
fn store_save_overwrites_existing() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();

    store.save("overwrite", Some("v1"), &[user_msg("first")]).unwrap();
    store.save("overwrite", Some("v2"), &[user_msg("second")]).unwrap();

    let loaded = store.load("overwrite").unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].content, "second");
}

#[test]
fn store_save_branch_isolation() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();

    store
        .save_branch("branch-1", Some("m"), &[user_msg("branch msg")], Some("parent"), Some(2))
        .unwrap();

    let loaded = store.load("branch-1").unwrap();
    assert_eq!(loaded.len(), 1);
}

#[test]
fn store_add_branch_updates_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();

    store.save("parent", None, &[user_msg("parent")]).unwrap();
    store.add_branch("parent", "child-1").unwrap();
    store.add_branch("parent", "child-2").unwrap();

    let meta = store
        .list_with_meta()
        .unwrap()
        .into_iter()
        .find(|m| m.id == "parent")
        .unwrap();
    assert_eq!(meta.branches.len(), 2);
    assert!(meta.branches.contains(&"child-1".to_string()));
    assert!(meta.branches.contains(&"child-2".to_string()));
}

#[test]
fn store_malformed_json_is_skipped_in_list() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();

    store.save("valid", None, &[user_msg("ok")]).unwrap();
    // Write a malformed JSON file directly.
    std::fs::write(tmp.path().join("invalid.json"), "not json").unwrap();

    let sessions = store.list_with_meta().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "valid");
}

#[test]
fn store_list_ignores_non_json_files() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();

    store.save("session", None, &[user_msg("ok")]).unwrap();
    std::fs::write(tmp.path().join("readme.txt"), "hello").unwrap();

    let sessions = store.list_with_meta().unwrap();
    assert_eq!(sessions.len(), 1);
}

#[test]
fn store_save_markdown_creates_file() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();

    store
        .save_markdown("md-session", Some("gpt-4"), &[user_msg("hi")])
        .unwrap();

    let path = tmp.path().join("md-session");
    assert!(path.exists());
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("# Chat Session"));
    assert!(contents.contains("**Model:** gpt-4"));
    assert!(contents.contains("User"));
    assert!(contents.contains("hi"));
}

#[test]
fn store_concurrent_saves_are_safe() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionStore::new(tmp.path().to_path_buf()).unwrap();

    std::thread::scope(|s| {
        for i in 0..5 {
            let store_ref = &store;
            s.spawn(move || {
                let id = format!("thread-{}", i);
                store_ref
                    .save(&id, None, &[user_msg(&format!("msg {}", i))])
                    .unwrap();
            });
        }
    });

    let sessions = store.list().unwrap();
    assert_eq!(sessions.len(), 5);
}
