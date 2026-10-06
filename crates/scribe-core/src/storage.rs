use std::future::Future;
use std::path::Path;

use turso::params::IntoParams;
use turso::{Builder, Connection, Database, Row};

use crate::model::{Dictation, Level, NewDictation, Outcome, Term, TermSource, TranscriptionUpdate};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("terme déjà présent dans le glossaire : {0}")]
    DuplicateTerm(String),
    #[error("base de données : {0}")]
    Sql(#[from] turso::Error),
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

/// v2: the history search matches `search_text`, filled in Rust (see [`search_text`]).
const SCHEMA_V2: &str = "ALTER TABLE dictations ADD COLUMN search_text TEXT NOT NULL DEFAULT '';";

const DICTATION_COLS: &str = "id, created_at, mode, app_name, audio_path, duration_ms, raw_text, final_text, \
     edited_text, level, transcriber, corrector, stt_ms, llm_ms, outcome, error";

/// Turso's API is async but its local I/O completes without a runtime: the storage stays synchronous
/// for its callers (behind `Mutex<Db>`) by driving each call to completion here.
fn block<F: Future>(f: F) -> F::Output {
    pollster::block_on(f)
}

/// What the history search matches: the three texts, lowercased in Rust because SQL `lower()` only
/// folds ASCII (« Été » vs « été »). The separator keeps a query from matching across two texts.
fn search_text(raw: Option<&str>, final_text: Option<&str>, edited: Option<&str>) -> String {
    [raw, final_text, edited].into_iter().flatten().map(str::to_lowercase).collect::<Vec<_>>().join("\u{1f}")
}

pub struct Db {
    conn: Connection,
    /// Owns the database the connection belongs to.
    _db: Database,
}

fn row_to_dictation(r: &Row) -> turso::Result<Dictation> {
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

fn row_to_term(r: &Row) -> turso::Result<Term> {
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

fn map_unique(e: StorageError, term: &str) -> StorageError {
    match e {
        StorageError::Sql(turso::Error::Constraint(_)) => StorageError::DuplicateTerm(term.to_string()),
        e => e,
    }
}

/// Runs `steps` (statements on `conn`) in one transaction: committed if they all succeed, else rolled back.
async fn transaction(conn: &Connection, steps: impl Future<Output = Result<()>>) -> Result<()> {
    conn.execute_batch("BEGIN").await?;
    match steps.await {
        Ok(()) => Ok(conn.execute_batch("COMMIT").await?),
        Err(e) => {
            // The step's error says what went wrong; a failed rollback would only hide it.
            let _ = conn.execute_batch("ROLLBACK").await;
            Err(e)
        }
    }
}

/// Every row of a query, mapped.
async fn all<T>(conn: &Connection, sql: &str, params: impl IntoParams, map: fn(&Row) -> turso::Result<T>) -> Result<Vec<T>> {
    let mut rows = conn.query(sql, params).await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(map(&row)?);
    }
    Ok(out)
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.to_str().ok_or_else(|| StorageError::Invalid(format!("chemin non UTF-8 : {}", path.display())))?;
        let db = Self::init(path)?;
        db.all("PRAGMA journal_mode=WAL", (), |_| Ok(()))?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(":memory:")
    }

    fn init(path: &str) -> Result<Self> {
        let db = block(Builder::new_local(path).build())?;
        let conn = db.connect()?;
        let db = Db { conn, _db: db };
        db.execute("PRAGMA foreign_keys = ON", ())?;
        db.migrate()?;
        Ok(db)
    }

    fn all<T>(&self, sql: &str, params: impl IntoParams, map: fn(&Row) -> turso::Result<T>) -> Result<Vec<T>> {
        block(all(&self.conn, sql, params, map))
    }

    fn first<T>(&self, sql: &str, params: impl IntoParams, map: fn(&Row) -> turso::Result<T>) -> Result<Option<T>> {
        Ok(self.all(sql, params, map)?.into_iter().next())
    }

    fn execute(&self, sql: &str, params: impl IntoParams) -> Result<u64> {
        Ok(block(self.conn.execute(sql, params))?)
    }

    fn migrate(&self) -> Result<()> {
        let version = self.first("PRAGMA user_version", (), |r| r.get::<i64>(0))?.unwrap_or(0);
        // One transaction per step: a failure part-way must not leave tables behind with an old user_version.
        let conn = &self.conn;
        if version < 1 {
            block(transaction(conn, async {
                conn.execute_batch(SCHEMA_V1).await?;
                Ok(conn.execute_batch("PRAGMA user_version = 1;").await?)
            }))?;
        }
        if version < 2 {
            block(transaction(conn, async {
                conn.execute_batch(SCHEMA_V2).await?;
                let rows = all(conn, "SELECT id, raw_text, final_text, edited_text FROM dictations", (), |r| {
                    Ok((r.get::<i64>(0)?, r.get::<Option<String>>(1)?, r.get::<Option<String>>(2)?, r.get::<Option<String>>(3)?))
                })
                .await?;
                for (id, raw, final_text, edited) in rows {
                    let text = search_text(raw.as_deref(), final_text.as_deref(), edited.as_deref());
                    conn.execute("UPDATE dictations SET search_text = ?2 WHERE id = ?1", (id, text)).await?;
                }
                Ok(conn.execute_batch("PRAGMA user_version = 2;").await?)
            }))?;
        }
        Ok(())
    }

    pub fn insert_dictation(&self, d: &NewDictation) -> Result<i64> {
        let search = search_text(d.raw_text.as_deref(), d.final_text.as_deref(), None);
        self.execute(
            "INSERT INTO dictations (created_at, mode, app_name, app_bundle_id, audio_path, duration_ms, raw_text, \
             final_text, level, transcriber, corrector, stt_ms, llm_ms, outcome, error, search_text) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            (
                d.created_at.as_str(), d.mode.as_str(), d.app_name.clone(), d.app_bundle_id.clone(), d.audio_path.clone(),
                d.duration_ms, d.raw_text.clone(), d.final_text.clone(), d.level.as_str(), d.transcriber.clone(),
                d.corrector.clone(), d.stt_ms, d.llm_ms, d.outcome.as_str(), d.error.clone(), search,
            ),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_dictation(&self, id: i64) -> Result<Option<Dictation>> {
        let sql = format!("SELECT {DICTATION_COLS} FROM dictations WHERE id = ?1");
        self.first(&sql, (id,), row_to_dictation)
    }

    pub fn list_dictations(&self, query: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Dictation>> {
        // instr(): literal substring match (no LIKE wildcards); both sides are lowercased in Rust.
        let needle = query.map(str::trim).filter(|q| !q.is_empty()).map(str::to_lowercase);
        let sql = format!(
            "SELECT {DICTATION_COLS} FROM dictations WHERE (?1 IS NULL OR instr(search_text, ?1) > 0) \
             ORDER BY id DESC LIMIT ?2 OFFSET ?3"
        );
        self.all(&sql, (needle, limit, offset), row_to_dictation)
    }

    /// Stores the user's correction; one equal to the final text (or None) clears it.
    pub fn set_edited_text(&self, id: i64, text: Option<&str>) -> Result<()> {
        let texts = self.first("SELECT raw_text, final_text FROM dictations WHERE id = ?1", (id,), |r| {
            Ok((r.get::<Option<String>>(0)?, r.get::<Option<String>>(1)?))
        })?;
        let Some((raw, final_text)) = texts else {
            return Ok(());
        };
        let edited = text.filter(|t| final_text.as_deref() != Some(*t));
        let search = search_text(raw.as_deref(), final_text.as_deref(), edited);
        self.execute(
            "UPDATE dictations SET edited_text = ?2, search_text = ?3 WHERE id = ?1",
            (id, edited.map(str::to_string), search),
        )?;
        Ok(())
    }

    /// Retranscription replaces the result: a user edit of the previous transcript is stale, so
    /// it is cleared (the history then shows the new final text).
    pub fn update_transcription(&self, id: i64, u: &TranscriptionUpdate) -> Result<()> {
        let search = search_text(u.raw_text.as_deref(), u.final_text.as_deref(), None);
        self.execute(
            "UPDATE dictations SET raw_text = ?2, final_text = ?3, edited_text = NULL, transcriber = ?4, corrector = ?5, \
             stt_ms = ?6, llm_ms = ?7, outcome = ?8, error = ?9, search_text = ?10 WHERE id = ?1",
            (
                id, u.raw_text.clone(), u.final_text.clone(), u.transcriber.clone(), u.corrector.clone(), u.stt_ms,
                u.llm_ms, u.outcome.as_str(), u.error.clone(), search,
            ),
        )?;
        Ok(())
    }

    pub fn delete_dictation(&self, id: i64) -> Result<Option<String>> {
        let audio = self.first("SELECT audio_path FROM dictations WHERE id = ?1", (id,), |r| r.get::<Option<String>>(0))?;
        self.execute("DELETE FROM dictations WHERE id = ?1", (id,))?;
        Ok(audio.flatten())
    }

    pub fn audio_to_purge(&self, older_than: &str) -> Result<Vec<(i64, String)>> {
        self.all(
            "SELECT id, audio_path FROM dictations WHERE audio_path IS NOT NULL AND created_at < ?1 ORDER BY id",
            (older_than,),
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    pub fn clear_audio_path(&self, id: i64) -> Result<()> {
        self.execute("UPDATE dictations SET audio_path = NULL WHERE id = ?1", (id,))?;
        Ok(())
    }

    pub fn list_terms(&self) -> Result<Vec<Term>> {
        self.all(
            "SELECT id, term, variants_json, note, source, use_count, last_used_at, created_at \
             FROM glossary_terms ORDER BY term COLLATE NOCASE",
            (),
            row_to_term,
        )
    }

    pub fn add_term(&self, term: &str, variants: &[String], note: Option<&str>, source: TermSource, now: &str) -> Result<i64> {
        let term = validate_term(term)?;
        let variants = serde_json::to_string(&clean_variants(variants, term)).expect("serialize variants");
        let note = note.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
        self.execute(
            "INSERT INTO glossary_terms (term, variants_json, note, source, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            (term, variants, note, source.as_str(), now),
        )
        .map_err(|e| map_unique(e, term))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_term(&self, id: i64, term: &str, variants: &[String], note: Option<&str>) -> Result<()> {
        let term = validate_term(term)?;
        let variants = serde_json::to_string(&clean_variants(variants, term)).expect("serialize variants");
        let note = note.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
        self.execute(
            "UPDATE glossary_terms SET term = ?2, variants_json = ?3, note = ?4 WHERE id = ?1",
            (id, term, variants, note),
        )
        .map_err(|e| map_unique(e, term))?;
        Ok(())
    }

    pub fn delete_term(&self, id: i64) -> Result<()> {
        self.execute("DELETE FROM glossary_terms WHERE id = ?1", (id,))?;
        Ok(())
    }

    pub fn bump_term_usage(&self, ids: &[i64], now: &str) -> Result<()> {
        let conn = &self.conn;
        block(transaction(conn, async {
            for &id in ids {
                conn.execute("UPDATE glossary_terms SET use_count = use_count + 1, last_used_at = ?2 WHERE id = ?1", (id, now))
                    .await?;
            }
            Ok(())
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::Mode;

    /// Runs SQL on the file through its own connection, as another program would.
    fn raw_sql(path: &Path, sql: &str) {
        block(async {
            let db = Builder::new_local(path.to_str().unwrap()).build().await.unwrap();
            db.connect().unwrap().execute_batch(sql).await.unwrap();
        })
    }

    fn raw_count(path: &Path, sql: &str) -> i64 {
        block(async {
            let db = Builder::new_local(path.to_str().unwrap()).build().await.unwrap();
            let conn = db.connect().unwrap();
            all(&conn, sql, (), |r| r.get::<i64>(0)).await.unwrap()[0]
        })
    }

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
        // raw « Il a 50 ans brut », final « Il a 50 ans »: a query never spans two texts.
        assert!(texts("brut il").is_empty());
    }

    #[test]
    fn search_follows_edits_and_retranscription() {
        let db = Db::open_in_memory().unwrap();
        let id = db.insert_dictation(&new_dictation("2026-10-05T10:00:00.000Z", "Bonjour", None)).unwrap();
        let found = |q: &str| db.list_dictations(Some(q), 10, 0).unwrap().len();
        db.set_edited_text(id, Some("Salut KUBERNETES")).unwrap();
        assert_eq!((found("kubernetes"), found("bonjour")), (1, 1), "the edit is searchable, the final text still is");
        db.set_edited_text(id, Some("Bonjour")).unwrap();
        assert_eq!(found("kubernetes"), 0, "an edit back to the final text is cleared");
        db.set_edited_text(id, Some("Salut Kubernetes")).unwrap();
        db.update_transcription(id, &TranscriptionUpdate {
            raw_text: Some("Été brut".into()),
            final_text: Some("Été".into()),
            transcriber: None,
            corrector: None,
            stt_ms: None,
            llm_ms: None,
            outcome: Outcome::Clipboard,
            error: None,
        }).unwrap();
        assert_eq!((found("kubernetes"), found("bonjour"), found("ÉTÉ")), (0, 0, 1), "retranscription replaces every text");
        db.set_edited_text(9_999, Some("x")).unwrap();
        assert_eq!(found("x"), 0, "an unknown id changes nothing");
    }

    #[test]
    fn opens_a_v1_database_written_by_rusqlite() {
        // Written by the rusqlite version (schema v1): one dictation with an edit, one term.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scribe.db");
        std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1-rusqlite.db"), &path).unwrap();
        for _ in 0..2 {
            let db = Db::open(&path).unwrap();
            assert_eq!(db.first("PRAGMA user_version", (), |r| r.get::<i64>(0)).unwrap(), Some(2));
            let all = db.list_dictations(None, 10, 0).unwrap();
            assert_eq!(all.len(), 1);
            assert_eq!((all[0].final_text.as_deref(), all[0].edited_text.as_deref()), (Some("Été à Paris"), Some("Été à LYON")));
            for q in ["lyon", "ÉTÉ À PARIS", "brut"] {
                assert_eq!(db.list_dictations(Some(q), 10, 0).unwrap().len(), 1, "existing rows are searchable: {q}");
            }
            let terms = db.list_terms().unwrap();
            assert_eq!((terms[0].term.as_str(), terms[0].variants.clone()), ("Kubernetes", vec!["cube ernetes".to_string()]));
        }
    }

    #[test]
    fn a_path_that_is_not_utf8_is_refused() {
        #[cfg(windows)]
        let path = {
            use std::os::windows::ffi::OsStringExt;
            std::path::PathBuf::from(std::ffi::OsString::from_wide(&[0xD800]))
        };
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&[0xff]))
        };
        assert!(matches!(Db::open(&path), Err(StorageError::Invalid(_))));
    }

    #[test]
    fn deleting_a_dictation_cascades_to_its_bench_runs() {
        let db = Db::open_in_memory().unwrap();
        let id = db.insert_dictation(&new_dictation("2026-10-05T10:00:00.000Z", "x", None)).unwrap();
        db.execute("INSERT INTO bench_runs (dictation_id, provider, model, created_at) VALUES (?1, 'p', 'm', 'now')", (id,)).unwrap();
        assert!(db.execute("INSERT INTO bench_runs (dictation_id, provider, model, created_at) VALUES (999, 'p', 'm', 'now')", ()).is_err(), "foreign keys are on");
        db.delete_dictation(id).unwrap();
        assert_eq!(db.first("SELECT count(*) FROM bench_runs", (), |r| r.get::<i64>(0)).unwrap(), Some(0));
    }

    #[test]
    fn failed_migration_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scribe.db");
        // A pre-existing table makes the last CREATE TABLE of the schema fail.
        raw_sql(&path, "CREATE TABLE bench_runs (x INTEGER);");
        assert!(Db::open(&path).is_err());
        assert_eq!(raw_count(&path, "SELECT count(*) FROM sqlite_schema WHERE name = 'dictations'"), 0);
        assert_eq!(raw_count(&path, "PRAGMA user_version"), 0);
        raw_sql(&path, "DROP TABLE bench_runs;");
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

    #[test]
    fn bump_term_usage_counts_each_listed_term_once_and_ignores_unknown_ids() {
        let db = Db::open_in_memory().unwrap();
        let now = "2026-10-05T10:00:00.000Z";
        let a = db.add_term("Tauri", &[], None, TermSource::Manual, now).unwrap();
        let b = db.add_term("Kubernetes", &[], None, TermSource::Mined, now).unwrap();
        let c = db.add_term("Svelte", &[], None, TermSource::Correction, now).unwrap();
        db.bump_term_usage(&[a, b, 9_999], "2026-10-05T11:00:00.000Z").unwrap();
        db.bump_term_usage(&[a], "2026-10-05T12:00:00.000Z").unwrap();
        db.bump_term_usage(&[], "2026-10-05T13:00:00.000Z").unwrap();
        let usage = |id: i64| {
            let t = db.list_terms().unwrap().into_iter().find(|t| t.id == id).unwrap();
            (t.use_count, t.last_used_at)
        };
        assert_eq!(usage(a), (2, Some("2026-10-05T12:00:00.000Z".into())));
        assert_eq!(usage(b), (1, Some("2026-10-05T11:00:00.000Z".into())));
        assert_eq!(usage(c), (0, None));
        assert_eq!(db.list_terms().unwrap().len(), 3, "an unknown id creates nothing");
    }
}
