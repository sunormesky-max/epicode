//! MemCard — small versioned persona/user/project/working blocks (≤2KB each).
//! Spike behind `EPICODE_MEMCARD=1`. Default OFF leaves production path untouched.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Hard cap per card content (bytes, UTF-8).
pub const MEMCARD_MAX_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardKind {
    Persona,
    User,
    Project,
    Working,
}

impl CardKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CardKind::Persona => "persona",
            CardKind::User => "user",
            CardKind::Project => "project",
            CardKind::Working => "working",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "persona" => Some(CardKind::Persona),
            "user" => Some(CardKind::User),
            "project" => Some(CardKind::Project),
            "working" => Some(CardKind::Working),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemCard {
    pub kind: CardKind,
    pub content: String,
    pub version: u64,
    pub updated_at: i64,
    /// Optional project scope for `project` cards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

impl MemCard {
    pub fn meta_json(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind.as_str(),
            "version": self.version,
            "updated_at": self.updated_at,
            "bytes": self.content.len(),
            "project": self.project,
        })
    }
}

#[derive(Debug)]
pub enum MemCardError {
    UnknownKind(String),
    TooLarge(usize),
    Sqlite(String),
}

impl fmt::Display for MemCardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemCardError::UnknownKind(k) => write!(f, "unknown card kind: {k}"),
            MemCardError::TooLarge(n) => {
                write!(f, "content exceeds {MEMCARD_MAX_BYTES} bytes (got {n})")
            }
            MemCardError::Sqlite(e) => write!(f, "sqlite: {e}"),
        }
    }
}

impl std::error::Error for MemCardError {}

/// In-memory + optional SQLite side table for MemCards.
pub struct MemCardStore {
    cards: RwLock<HashMap<(CardKind, String), MemCard>>,
    db_path: Option<PathBuf>,
}

impl MemCardStore {
    pub fn in_memory() -> Self {
        Self {
            cards: RwLock::new(HashMap::new()),
            db_path: None,
        }
    }

    pub fn open(data_dir: &Path) -> Self {
        let db_path = data_dir.join("memcards.sqlite");
        let store = Self {
            cards: RwLock::new(HashMap::new()),
            db_path: Some(db_path.clone()),
        };
        if let Err(e) = store.init_db() {
            tracing::warn!("[MemCard] sqlite init failed: {e} — using memory only");
        } else if let Err(e) = store.load_from_db() {
            tracing::warn!("[MemCard] sqlite load failed: {e}");
        }
        store
    }

    fn scope_key(project: Option<&str>) -> String {
        project.unwrap_or("").to_string()
    }

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn init_db(&self) -> Result<(), MemCardError> {
        let Some(path) = &self.db_path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn =
            rusqlite::Connection::open(path).map_err(|e| MemCardError::Sqlite(e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memcards (
                kind TEXT NOT NULL,
                project TEXT NOT NULL DEFAULT '',
                content TEXT NOT NULL,
                version INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (kind, project)
            );",
        )
        .map_err(|e| MemCardError::Sqlite(e.to_string()))?;
        Ok(())
    }

    fn load_from_db(&self) -> Result<(), MemCardError> {
        let Some(path) = &self.db_path else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }
        let conn =
            rusqlite::Connection::open(path).map_err(|e| MemCardError::Sqlite(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT kind, project, content, version, updated_at FROM memcards")
            .map_err(|e| MemCardError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let kind_s: String = row.get(0)?;
                let project: String = row.get(1)?;
                let content: String = row.get(2)?;
                let version: i64 = row.get(3)?;
                let updated_at: i64 = row.get(4)?;
                Ok((kind_s, project, content, version as u64, updated_at))
            })
            .map_err(|e| MemCardError::Sqlite(e.to_string()))?;
        let mut map = self.cards.write();
        for row in rows {
            let (kind_s, project, content, version, updated_at) =
                row.map_err(|e| MemCardError::Sqlite(e.to_string()))?;
            let Some(kind) = CardKind::parse(&kind_s) else {
                continue;
            };
            let proj = if project.is_empty() {
                None
            } else {
                Some(project.clone())
            };
            map.insert(
                (kind, project),
                MemCard {
                    kind,
                    content,
                    version,
                    updated_at,
                    project: proj,
                },
            );
        }
        Ok(())
    }

    fn persist(&self, card: &MemCard) -> Result<(), MemCardError> {
        let Some(path) = &self.db_path else {
            return Ok(());
        };
        let conn =
            rusqlite::Connection::open(path).map_err(|e| MemCardError::Sqlite(e.to_string()))?;
        conn.execute(
            "INSERT INTO memcards (kind, project, content, version, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(kind, project) DO UPDATE SET
               content=excluded.content,
               version=excluded.version,
               updated_at=excluded.updated_at",
            rusqlite::params![
                card.kind.as_str(),
                card.project.as_deref().unwrap_or(""),
                card.content,
                card.version as i64,
                card.updated_at,
            ],
        )
        .map_err(|e| MemCardError::Sqlite(e.to_string()))?;
        Ok(())
    }

    pub fn get(&self, kind: CardKind, project: Option<&str>) -> Option<MemCard> {
        let key = (kind, Self::scope_key(project));
        self.cards.read().get(&key).cloned()
    }

    pub fn set(
        &self,
        kind: CardKind,
        content: String,
        project: Option<&str>,
    ) -> Result<MemCard, MemCardError> {
        if content.len() > MEMCARD_MAX_BYTES {
            return Err(MemCardError::TooLarge(content.len()));
        }
        let key = (kind, Self::scope_key(project));
        let mut map = self.cards.write();
        let version = map.get(&key).map(|c| c.version + 1).unwrap_or(1);
        let card = MemCard {
            kind,
            content,
            version,
            updated_at: Self::now(),
            project: project.map(|s| s.to_string()).filter(|s| !s.is_empty()),
        };
        map.insert(key, card.clone());
        drop(map);
        let _ = self.persist(&card);
        Ok(card)
    }

    pub fn set_parsed(
        &self,
        kind: &str,
        content: String,
        project: Option<&str>,
    ) -> Result<MemCard, MemCardError> {
        let kind = CardKind::parse(kind).ok_or_else(|| MemCardError::UnknownKind(kind.into()))?;
        self.set(kind, content, project)
    }

    pub fn get_parsed(
        &self,
        kind: &str,
        project: Option<&str>,
    ) -> Result<Option<MemCard>, MemCardError> {
        let kind = CardKind::parse(kind).ok_or_else(|| MemCardError::UnknownKind(kind.into()))?;
        Ok(self.get(kind, project))
    }

    /// Meta for all cards (no content) — for status envelope.
    pub fn meta_all(&self) -> Vec<serde_json::Value> {
        let map = self.cards.read();
        let mut out: Vec<_> = map.values().map(|c| c.meta_json()).collect();
        out.sort_by(|a, b| {
            a["kind"]
                .as_str()
                .unwrap_or("")
                .cmp(b["kind"].as_str().unwrap_or(""))
        });
        out
    }

    pub fn count(&self) -> usize {
        self.cards.read().len()
    }
}

pub type SharedMemCards = Arc<MemCardStore>;

/// Deterministic preference grader: true if haystack contains any expected token
/// (ASCII case-insensitive). No LLM — used by MemCard preference harness.
pub fn pref_answer_hits(haystack: &str, expected_tokens: &[&str]) -> bool {
    let h = haystack.to_lowercase();
    expected_tokens
        .iter()
        .any(|t| h.contains(&t.to_lowercase()))
}

/// Extract preference lines from a MemCard that share tokens with `query`.
/// Falls back to full content when no line matches (still tiny vs search payloads).
pub fn answer_prefs_from_card(content: &str, query: &str) -> String {
    let q_lower = query.to_lowercase();
    let q_parts: Vec<&str> = q_lower
        .split(|c: char| c.is_ascii_whitespace() || c.is_ascii_punctuation())
        .filter(|t| t.len() >= 2)
        .collect();
    let mut lines: Vec<&str> = Vec::new();
    for line in content.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let ll = l.to_lowercase();
        if q_parts.iter().any(|t| ll.contains(t)) {
            lines.push(l);
        }
    }
    if lines.is_empty() {
        content.to_string()
    } else {
        lines.join("\n")
    }
}

pub fn memcard_enabled() -> bool {
    crate::engine::spike_flags::env_flag("EPICODE_MEMCARD")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_version_increments() {
        let store = MemCardStore::in_memory();
        let c1 = store
            .set(CardKind::Persona, "I am a careful coder.".into(), None)
            .unwrap();
        assert_eq!(c1.version, 1);
        let got = store.get(CardKind::Persona, None).unwrap();
        assert_eq!(got.content, "I am a careful coder.");
        assert_eq!(got.version, 1);

        let c2 = store
            .set(CardKind::Persona, "I prefer Rust.".into(), None)
            .unwrap();
        assert_eq!(c2.version, 2);
        assert_eq!(store.get(CardKind::Persona, None).unwrap().version, 2);
    }

    #[test]
    fn rejects_oversized_content() {
        let store = MemCardStore::in_memory();
        let big = "x".repeat(MEMCARD_MAX_BYTES + 1);
        let err = store.set(CardKind::User, big, None).unwrap_err();
        assert!(matches!(err, MemCardError::TooLarge(_)));
    }

    #[test]
    fn project_scoped_cards_are_independent() {
        let store = MemCardStore::in_memory();
        store
            .set(CardKind::Project, "epicode prefs".into(), Some("epicode"))
            .unwrap();
        store
            .set(CardKind::Project, "other prefs".into(), Some("other"))
            .unwrap();
        assert_eq!(
            store
                .get(CardKind::Project, Some("epicode"))
                .unwrap()
                .content,
            "epicode prefs"
        );
        assert_eq!(
            store.get(CardKind::Project, Some("other")).unwrap().content,
            "other prefs"
        );
    }

    #[test]
    fn sqlite_roundtrip() {
        let dir = tempfile_dir();
        let store = MemCardStore::open(&dir);
        store
            .set(CardKind::User, "likes dark mode".into(), None)
            .unwrap();
        drop(store);
        let store2 = MemCardStore::open(&dir);
        let got = store2.get(CardKind::User, None).unwrap();
        assert_eq!(got.content, "likes dark mode");
        assert_eq!(got.version, 1);
    }

    /// Shared fixture: ≥10 bilingual prefs + questions + expected tokens.
    pub(crate) fn preference_fixture() -> (String, Vec<(&'static str, Vec<&'static str>)>) {
        let card = r#"User preferences:
1. UI theme: dark mode
2. Preferred editor: neovim
3. Code comments language: 中文
4. Shell: fish
5. Package manager: pnpm
6. Test runner: cargo nextest
7. Commit style: conventional commits
8. Reply length: terse
9. Keyboard layout: colemak
10. Favorite snacks: 辣条
"#
        .to_string();
        let qa = vec![
            ("What UI theme does the user prefer?", vec!["dark"]),
            ("Which editor does the user prefer?", vec!["neovim"]),
            ("What language for code comments?", vec!["中文"]),
            ("Which shell does the user use?", vec!["fish"]),
            ("Preferred package manager?", vec!["pnpm"]),
            ("What test runner is preferred?", vec!["nextest"]),
            ("Commit message style?", vec!["conventional"]),
            ("Does the user want long or terse replies?", vec!["terse"]),
            ("Keyboard layout preference?", vec!["colemak"]),
            ("Favorite snacks?", vec!["辣条"]),
        ];
        (card, qa)
    }

    /// Round-2 S2 harness: MemCard answers 10/10 prefs from card alone, with tiny bytes.
    /// Baseline is a noisy lexical corpus (simulates vector/SMRP top-k drowning).
    #[test]
    fn spike_s2_memcard_preference_harness_beats_noisy_baseline() {
        let (card, qa) = preference_fixture();
        assert!(card.len() <= MEMCARD_MAX_BYTES);
        assert!(qa.len() >= 10);

        let store = MemCardStore::in_memory();
        store.set(CardKind::User, card.clone(), None).unwrap();
        let got = store.get(CardKind::User, None).unwrap();

        // --- MemCard path: answer from blocks ONLY ---
        let mut mem_hits = 0usize;
        let mut mem_bytes = 0usize;
        for (q, expected) in &qa {
            let ans = answer_prefs_from_card(&got.content, q);
            mem_bytes += ans.len();
            if pref_answer_hits(&ans, expected) {
                mem_hits += 1;
            }
        }

        // --- Baseline: noisy corpus + lexical top-k (no MemCard) ---
        // Seed same prefs as "memories" plus long noise that shares question keywords.
        let mut corpus: Vec<String> = Vec::new();
        for (i, line) in card.lines().skip(1).enumerate() {
            let l = line.trim();
            if !l.is_empty() {
                corpus.push(format!("[preference] {l} #pref-{i}"));
            }
        }
        let noise = "Lorem ipsum editor theme shell package manager test runner commit keyboard snacks reply language comments UI user prefers vscode emacs bash npm yarn jest mocha long verbose qwerty chocolate cookies. ".repeat(50);
        for i in 0..12 {
            // Noise deliberately mentions question keywords with WRONG answers.
            corpus.push(format!(
                "{noise} distractor-{i}: UI theme light mode; editor vscode; shell bash; package manager npm; test runner jest; commit style freeform; reply length verbose; keyboard qwerty; snacks chocolate; comments English."
            ));
        }

        let mut base_hits = 0usize;
        let mut base_bytes = 0usize;
        for (q, expected) in &qa {
            let q_lower = q.to_lowercase();
            let q_parts: Vec<&str> = q_lower
                .split(|c: char| c.is_ascii_whitespace() || c.is_ascii_punctuation())
                .filter(|t| t.len() >= 3)
                .collect();
            let mut scored: Vec<(usize, &String)> = corpus
                .iter()
                .map(|doc| {
                    let dl = doc.to_lowercase();
                    let score = q_parts.iter().filter(|t| dl.contains(*t)).count();
                    (score, doc)
                })
                .collect();
            scored.sort_by_key(|a| std::cmp::Reverse(a.0));
            // top-k=3 retrieval payload (bytes) + containment grade
            let topk: Vec<&String> = scored.into_iter().take(3).map(|(_, d)| d).collect();
            let payload = topk
                .iter()
                .map(|d| d.as_str())
                .collect::<Vec<_>>()
                .join("\n---\n");
            base_bytes += payload.len();
            if pref_answer_hits(&payload, expected) {
                base_hits += 1;
            }
        }

        eprintln!(
            "[spike-s2-harness] memcard hits={mem_hits}/{} bytes={mem_bytes}; baseline hits={base_hits}/{} bytes={base_bytes}; ratio={:.1}%",
            qa.len(),
            qa.len(),
            mem_bytes as f64 * 100.0 / base_bytes.max(1) as f64
        );

        assert_eq!(
            mem_hits,
            qa.len(),
            "MemCard must answer all prefs from card alone"
        );
        assert!(
            mem_hits >= base_hits,
            "MemCard accuracy {mem_hits} must be ≥ baseline {base_hits}"
        );
        // Win: MemCard answer bytes ≤ 20% of baseline top-k payload.
        assert!(
            mem_bytes * 100 / base_bytes.max(1) <= 20,
            "MemCard bytes {mem_bytes} should be ≤20% of baseline {base_bytes} (got {}%)",
            mem_bytes * 100 / base_bytes.max(1)
        );
        // Document baseline failure mode when noise drowns prefs.
        if base_hits < qa.len() {
            eprintln!(
                "[spike-s2-harness] baseline FAILED {} of {} prefs — long distractors sharing question keywords drown preference memories in top-k",
                qa.len() - base_hits,
                qa.len()
            );
        }
    }

    fn tempfile_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "epicode-memcard-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&p);
        p
    }
}
