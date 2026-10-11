use rusqlite::Connection;
use said_core::SCHEMA;

#[test]
fn originals_are_immutable_and_search_tracks_edits_and_deletion() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(SCHEMA).unwrap();
    db.execute("INSERT INTO utterances(id, audio_id, started_at, duration_ms, engine) VALUES ('synthetic', 'synthetic', 0, 0, 'fake')", []).unwrap();
    db.execute("UPDATE utterances SET raw_text = 'no no synthetic', text = 'no no synthetic', status = 'done' WHERE id = 'synthetic'", []).unwrap();
    let matches = |query: &str| -> i64 {
        db.query_row(
            "SELECT count(*) FROM utterances_fts WHERE utterances_fts MATCH ?1",
            [query],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(matches("synthetic"), 1);
    assert!(
        db.execute("UPDATE utterances SET raw_text = 'rewritten'", [])
            .is_err()
    );
    assert!(
        db.execute("UPDATE utterances SET audio_id = 'other'", [])
            .is_err()
    );
    db.execute("UPDATE utterances SET text = 'edited'", [])
        .unwrap();
    assert_eq!(matches("edited"), 1);
    assert_eq!(matches("synthetic"), 1);
    db.execute("DELETE FROM utterances", []).unwrap();
    assert_eq!(matches("edited"), 0);
    assert_eq!(matches("synthetic"), 0);
}
