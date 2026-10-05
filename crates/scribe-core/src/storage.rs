use std::path::Path;

use rusqlite::functions::FunctionFlags;
use rusqlite::{params, Connection, ErrorCode, OptionalExtension};

use crate::model::{Dictation, Level, NewDictation, Outcome, Term, TermSource, TranscriptionUpdate};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("terme déjà présent dans le glossaire : {0}")]
    DuplicateTerm(String),
    #[error("base de données : {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("données invalides : {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

const SCHEMA_V1: &str = r#"
CREATE TABLE dictations (
  id INTEGER PRIMARY KEY,
  created_at TEXT NOT NULL,
  mode TEXT NOT NULL,
  app_name TEXT,
  app_bundle_id TEXT,
  audio_path TEXT,
  duration_ms INTEGER NOT NULL,
  raw_text TEXT,
  final_text TEXT,
  edited_text TEXT,
  level TEXT NOT NULL,
  transcriber TEXT,
  corrector TEXT,
  stt_ms INTEGER,
  llm_ms INTEGER,
  outcome TEXT NOT NULL,
  error TEXT
);
CREATE INDEX idx_dictations_created ON dictations(created_at);
CREATE TABLE glossary_terms (
  id INTEGER PRIMARY KEY,
  term TEXT NOT NULL UNIQUE COLLATE NOCASE,
  variants_json TEXT NOT NULL DEFAULT '[]',
  note TEXT,
  source TEXT NOT NULL,
  use_count INTEGER NOT NULL DEFAULT 0,
  last_used_at TEXT,
  created_at TEXT NOT NULL
);
CREATE TABLE suggestions (
  id INTEGER PRIMARY KEY,
  term TEXT NOT NULL,
  variants_json TEXT NOT NULL DEFAULT '[]',
  evidence_json TEXT NOT NULL DEFAULT '[]',
  source TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  created_at TEXT NOT NULL
);
CREATE TABLE bench_runs (
  id INTEGER PRIMARY KEY,
  dictation_id INTEGER NOT NULL REFERENCES dictations(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  text TEXT,
  latency_ms INTEGER,
  created_at TEXT NOT NULL
);
"#;

const DICTATION_COLS: &str = "id, created_at, mode, app_name, audio_path, duration_ms, raw_text, final_text, \
     edited_text, level, transcriber, corrector, stt_ms, llm_ms, outcome, error";

pub struct Db {
    conn: Connection,
}

fn row_to_dictation(r: &rusqlite::Row) -> rusqlite::Result<Dictation> {
    let level: String = r.get(9)?;
    let outcome: String = r.get(14)?;
    Ok(Dictation {
        id: r.get(0)?,
        created_at: r.get(1)?,
        mode: r.get(2)?,
        app_name: r.get(3)?,
        audio_path: r.get(4)?,
        duration_ms: r.get(5)?,
        raw_text: r.get(6)?,
        final_text: r.get(7)?,
        edited_text: r.get(8)?,
        level: Level::parse(&level).unwrap_or(Level::Raw),
        transcriber: r.get(10)?,
        corrector: r.get(11)?,
        stt_ms: r.get(12)?,
        llm_ms: r.get(13)?,
        outcome: Outcome::parse(&outcome).unwrap_or(Outcome::Error),
        error: r.get(15)?,
    })
}

fn row_to_term(r: &rusqlite::Row) -> rusqlite::Result<Term> {
    let variants_json: String = r.get(2)?;
    let source: String = r.get(4)?;
    Ok(Term {
        id: r.get(0)?,
        term: r.get(1)?,
        variants: serde_json::from_str(&variants_json).unwrap_or_default(),
        note: r.get(3)?,
        source: TermSource::parse(&source).unwrap_or(TermSource::Manual),
        use_count: r.get(5)?,
        last_used_at: r.get(6)?,
        created_at: r.get(7)?,
    })
}

/// Trims, drops empty entries, entries equal to the term and case-insensitive duplicates.
fn clean_variants(variants: &[String], term: &str) -> Vec<String> {
    let mut seen: Vec<String> = vec![term.to_lowercase()];
    let mut out = Vec::new();
    for v in variants {
        let v = v.trim();
        if v.is_empty() || seen.contains(&v.to_lowercase()) {
            continue;
        }
        seen.push(v.to_lowercase());
        out.push(v.to_string());
    }
    out
}

fn validate_term(term: &str) -> Result<&str> {
    let term = term.trim();
    if term.is_empty() {
        return Err(StorageError::Invalid("terme vide".into()));
    }
    Ok(term)
}

fn map_unique(e: rusqlite::Error, term: &str) -> StorageError {
    match &e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == ErrorCode::ConstraintViolation => {
            StorageError::DuplicateTerm(term.to_string())
        }
        _ => StorageError::Sql(e),
    }
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        // Unicode-aware lowercase: SQLite's LIKE/lower() only fold ASCII ("Été" vs "été").
        conn.create_scalar_function(
            "scribe_fold",
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |ctx| Ok(ctx.get::<Option<String>>(0)?.map(|t| t.to_lowercase())),
        )?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            // One transaction: a failure part-way must not leave tables behind with user_version 0.
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(SCHEMA_V1)?;
            tx.execute_batch("PRAGMA user_version = 1;")?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn insert_dictation(&self, d: &NewDictation) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO dictations (created_at, mode, app_name, app_bundle_id, audio_path, duration_ms, raw_text, \
             final_text, level, transcriber, corrector, stt_ms, llm_ms, outcome, error) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                d.created_at, d.mode.as_str(), d.app_name, d.app_bundle_id, d.audio_path, d.duration_ms,
                d.raw_text, d.final_text, d.level.as_str(), d.transcriber, d.corrector, d.stt_ms, d.llm_ms,
                d.outcome.as_str(), d.error
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_dictation(&self, id: i64) -> Result<Option<Dictation>> {
        let sql = format!("SELECT {DICTATION_COLS} FROM dictations WHERE id = ?1");
        Ok(self.conn.query_row(&sql, params![id], row_to_dictation).optional()?)
    }

    pub fn list_dictations(&self, query: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Dictation>> {
        // instr() on folded text: literal substring match (no LIKE wildcards), Unicode case-insensitive.
        let needle = query.map(str::trim).filter(|q| !q.is_empty()).map(str::to_lowercase);
        let sql = format!(
            "SELECT {DICTATION_COLS} FROM dictations \
             WHERE (?1 IS NULL OR instr(scribe_fold(raw_text), ?1) > 0 OR instr(scribe_fold(final_text), ?1) > 0 \
             OR instr(scribe_fold(edited_text), ?1) > 0) \
             ORDER BY id DESC LIMIT ?2 OFFSET ?3"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![needle, limit, offset], row_to_dictation)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_edited_text(&self, id: i64, text: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE dictations SET edited_text = CASE WHEN ?2 IS NULL OR ?2 = final_text THEN NULL ELSE ?2 END \
             WHERE id = ?1",
            params![id, text],
        )?;
        Ok(())
    }

    /// Retranscription replaces the result: a user edit of the previous transcript is stale, so
    /// it is cleared (the history then shows the new final text).
    pub fn update_transcription(&self, id: i64, u: &TranscriptionUpdate) -> Result<()> {
        self.conn.execute(
            "UPDATE dictations SET raw_text = ?2, final_text = ?3, edited_text = NULL, transcriber = ?4, corrector = ?5, \
             stt_ms = ?6, llm_ms = ?7, outcome = ?8, error = ?9 WHERE id = ?1",
            params![id, u.raw_text, u.final_text, u.transcriber, u.corrector, u.stt_ms, u.llm_ms,
                    u.outcome.as_str(), u.error],
        )?;
        Ok(())
    }

    pub fn delete_dictation(&self, id: i64) -> Result<Option<String>> {
        let audio: Option<Option<String>> = self
            .conn
            .query_row("SELECT audio_path FROM dictations WHERE id = ?1", params![id], |r| r.get(0))
            .optional()?;
        self.conn.execute("DELETE FROM dictations WHERE id = ?1", params![id])?;
        Ok(audio.flatten())
    }

    pub fn audio_to_purge(&self, older_than: &str) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, audio_path FROM dictations WHERE audio_path IS NOT NULL AND created_at < ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![older_than], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn clear_audio_path(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE dictations SET audio_path = NULL WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_terms(&self) -> Result<Vec<Term>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, term, variants_json, note, source, use_count, last_used_at, created_at \
             FROM glossary_terms ORDER BY term COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], row_to_term)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn add_term(&self, term: &str, variants: &[String], note: Option<&str>, source: TermSource, now: &str) -> Result<i64> {
        let term = validate_term(term)?;
        let variants = serde_json::to_string(&clean_variants(variants, term)).expect("serialize variants");
        let note = note.map(str::trim).filter(|n| !n.is_empty());
        self.conn
            .execute(
                "INSERT INTO glossary_terms (term, variants_json, note, source, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![term, variants, note, source.as_str(), now],
            )
            .map_err(|e| map_unique(e, term))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_term(&self, id: i64, term: &str, variants: &[String], note: Option<&str>) -> Result<()> {
        let term = validate_term(term)?;
        let variants = serde_json::to_string(&clean_variants(variants, term)).expect("serialize variants");
        let note = note.map(str::trim).filter(|n| !n.is_empty());
        self.conn
            .execute(
                "UPDATE glossary_terms SET term = ?2, variants_json = ?3, note = ?4 WHERE id = ?1",
                params![id, term, variants, note],
            )
            .map_err(|e| map_unique(e, term))?;
        Ok(())
    }

    pub fn delete_term(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM glossary_terms WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn bump_term_usage(&self, ids: &[i64], now: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for id in ids {
            tx.execute(
                "UPDATE glossary_terms SET use_count = use_count + 1, last_used_at = ?2 WHERE id = ?1",
                params![id, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::Mode;

    fn new_dictation(created_at: &str, final_text: &str, audio: Option<&str>) -> NewDictation {
        NewDictation {
            created_at: created_at.into(),
            mode: Mode::Hold,
            app_name: Some("Code".into()),
            app_bundle_id: None,
            audio_path: audio.map(String::from),
            duration_ms: 1200,
            raw_text: Some(format!("{final_text} brut")),
            final_text: Some(final_text.into()),
            level: Level::Formatted,
            transcriber: Some("gpt-4o-transcribe".into()),
            corrector: Some("claude-opus-5-5".into()),
            stt_ms: Some(800),
            llm_ms: Some(900),
            outcome: Outcome::Pasted,
            error: None,
        }
    }

    #[test]
    fn migration_is_idempotent_and_data_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scribe.db");
        {
            let db = Db::open(&path).unwrap();
            db.insert_dictation(&new_dictation("2026-10-05T10:00:00.000Z", "Bonjour", None)).unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.list_dictations(None, 10, 0).unwrap().len(), 1);
    }

    #[test]
    fn list_is_newest_first_and_search_matches_text() {
        let db = Db::open_in_memory().unwrap();
        db.insert_dictation(&new_dictation("2026-10-05T10:00:00.000Z", "Premier Kubernetes", None)).unwrap();
        db.insert_dictation(&new_dictation("2026-10-05T11:00:00.000Z", "Second", None)).unwrap();
        let all = db.list_dictations(None, 10, 0).unwrap();
        assert_eq!(all[0].final_text.as_deref(), Some("Second"));
        let found = db.list_dictations(Some("kubernetes"), 10, 0).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(db.list_dictations(Some("  "), 10, 0).unwrap().len(), 2);
        assert_eq!(db.list_dictations(None, 1, 1).unwrap()[0].final_text.as_deref(), Some("Premier Kubernetes"));
    }

    #[test]
    fn search_is_literal_and_unicode_case_insensitive() {
        let db = Db::open_in_memory().unwrap();
        db.insert_dictation(&new_dictation("2026-10-05T10:00:00.000Z", "Été chargé", None)).unwrap();
        db.insert_dictation(&new_dictation("2026-10-05T11:00:00.000Z", "Remise de 50% sur 500", None)).unwrap();
        db.insert_dictation(&new_dictation("2026-10-05T12:00:00.000Z", "Il a 50 ans", None)).unwrap();
        db.insert_dictation(&new_dictation("2026-10-05T13:00:00.000Z", "snake_case", None)).unwrap();
        db.insert_dictation(&new_dictation("2026-10-05T14:00:00.000Z", "snakeXcase", None)).unwrap();
        let texts = |q: &str| -> Vec<String> {
            db.list_dictations(Some(q), 10, 0).unwrap().into_iter().filter_map(|d| d.final_text).collect()
        };
        assert_eq!(texts("été"), vec!["Été chargé".to_string()]);
        assert_eq!(texts("ÉTÉ CHARGÉ"), vec!["Été chargé".to_string()]);
        assert_eq!(texts("50%"), vec!["Remise de 50% sur 500".to_string()]);
        assert_eq!(texts("snake_case"), vec!["snake_case".to_string()]);
    }

    #[test]
    fn failed_migration_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scribe.db");
        // A pre-existing table makes the last CREATE TABLE of the schema fail.
        Connection::open(&path).unwrap().execute_batch("CREATE TABLE bench_runs (x INTEGER);").unwrap();
        assert!(Db::open(&path).is_err());
        let conn = Connection::open(&path).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'dictations'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        conn.execute_batch("DROP TABLE bench_runs;").unwrap();
        drop(conn);
        let db = Db::open(&path).unwrap();
        db.insert_dictation(&new_dictation("2026-10-05T10:00:00.000Z", "ok", None)).unwrap();
    }

    #[test]
    fn retranscription_clears_stale_edit() {
        let db = Db::open_in_memory().unwrap();
        let id = db.insert_dictation(&new_dictation("2026-10-05T10:00:00.000Z", "Ancien", None)).unwrap();
        db.set_edited_text(id, Some("X")).unwrap();
        db.update_transcription(id, &TranscriptionUpdate {
            raw_text: Some("brut".into()),
            final_text: Some("X".into()),
            transcriber: None,
            corrector: None,
            stt_ms: None,
            llm_ms: None,
            outcome: Outcome::Clipboard,
            error: None,
        }).unwrap();
        let d = db.get_dictation(id).unwrap().unwrap();
        assert_eq!(d.edited_text, None);
        assert_eq!(d.best_text(), Some("X"));
    }

    #[test]
    fn edited_text_equal_to_final_is_stored_as_null() {
        let db = Db::open_in_memory().unwrap();
        let id = db.insert_dictation(&new_dictation("2026-10-05T10:00:00.000Z", "Texte", None)).unwrap();
        db.set_edited_text(id, Some("Texte corrigé")).unwrap();
        let d = db.get_dictation(id).unwrap().unwrap();
        assert_eq!(d.edited_text.as_deref(), Some("Texte corrigé"));
        assert_eq!(d.best_text(), Some("Texte corrigé"));
        db.set_edited_text(id, Some("Texte")).unwrap();
        assert_eq!(db.get_dictation(id).unwrap().unwrap().edited_text, None);
    }

    #[test]
    fn update_transcription_replaces_results() {
        let db = Db::open_in_memory().unwrap();
        let mut nd = new_dictation("2026-10-05T10:00:00.000Z", "x", Some("a.wav"));
        nd.outcome = Outcome::Error;
        nd.raw_text = None;
        nd.final_text = None;
        nd.error = Some("réseau".into());
        let id = db.insert_dictation(&nd).unwrap();
        db.update_transcription(id, &TranscriptionUpdate {
            raw_text: Some("brut".into()),
            final_text: Some("Final.".into()),
            transcriber: Some("t".into()),
            corrector: Some("c".into()),
            stt_ms: Some(1),
            llm_ms: Some(2),
            outcome: Outcome::Clipboard,
            error: None,
        }).unwrap();
        let d = db.get_dictation(id).unwrap().unwrap();
        assert_eq!((d.final_text.as_deref(), d.outcome, d.error), (Some("Final."), Outcome::Clipboard, None));
    }

    #[test]
    fn delete_returns_audio_path_and_purge_lists_old_audio() {
        let db = Db::open_in_memory().unwrap();
        let old = db.insert_dictation(&new_dictation("2026-08-01T10:00:00.000Z", "vieux", Some("old.wav"))).unwrap();
        db.insert_dictation(&new_dictation("2026-08-01T10:00:00.000Z", "sans audio", None)).unwrap();
        let recent = db.insert_dictation(&new_dictation("2026-10-05T10:00:00.000Z", "récent", Some("new.wav"))).unwrap();
        assert_eq!(db.audio_to_purge("2026-09-05T00:00:00.000Z").unwrap(), vec![(old, "old.wav".to_string())]);
        db.clear_audio_path(old).unwrap();
        assert!(db.audio_to_purge("2026-09-05T00:00:00.000Z").unwrap().is_empty());
        assert_eq!(db.delete_dictation(recent).unwrap(), Some("new.wav".to_string()));
        assert!(db.get_dictation(recent).unwrap().is_none());
    }

    #[test]
    fn terms_crud_with_case_insensitive_uniqueness_and_clean_variants() {
        let db = Db::open_in_memory().unwrap();
        let now = "2026-10-05T10:00:00.000Z";
        let id = db.add_term(
            " Kubernetes ",
            &["cube ernetes".into(), " ".into(), "kubernetes".into(), "Cube Ernetes".into()],
            Some("orchestrateur"),
            TermSource::Manual,
            now,
        ).unwrap();
        let t = &db.list_terms().unwrap()[0];
        assert_eq!(t.term, "Kubernetes");
        assert_eq!(t.variants, vec!["cube ernetes".to_string()]);
        assert!(matches!(db.add_term("kubernetes", &[], None, TermSource::Manual, now), Err(StorageError::DuplicateTerm(_))));
        assert!(matches!(db.add_term("  ", &[], None, TermSource::Manual, now), Err(StorageError::Invalid(_))));
        db.update_term(id, "Kubernetes", &["kubernetis".into()], None).unwrap();
        assert_eq!(db.list_terms().unwrap()[0].variants, vec!["kubernetis".to_string()]);
        db.bump_term_usage(&[id], "2026-10-05T12:00:00.000Z").unwrap();
        db.bump_term_usage(&[id], "2026-10-05T13:00:00.000Z").unwrap();
        let t = &db.list_terms().unwrap()[0];
        assert_eq!((t.use_count, t.last_used_at.as_deref()), (2, Some("2026-10-05T13:00:00.000Z")));
        db.delete_term(id).unwrap();
        assert!(db.list_terms().unwrap().is_empty());
    }
}
