use rusqlite::{params, Connection};
use std::path::Path;

pub struct HelpDb {
    conn: Connection,
}

pub struct SearchResult {
    pub topic_id: String,
    pub title: String,
    pub excerpt: String,
}

pub struct TopicRow {
    pub topic_id: String,
    pub title: String,
    pub content: String,
    pub category: String,
    pub version: String,
}

impl HelpDb {
    pub fn new(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("Failed to open database: {e}"))?;

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| format!("Failed to set journal_mode: {e}"))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| format!("Failed to set synchronous: {e}"))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS topics USING fts5(
                topic_id,
                title,
                content,
                category,
                version,
                tokenize = 'unicode61'
            );",
        )
        .map_err(|e| format!("Failed to create tables: {e}"))?;

        Ok(HelpDb { conn })
    }

    pub fn search(&self, query: &str, category: &str, limit: u32) -> Result<Vec<SearchResult>, String> {
        let fts_query = query
            .split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" AND ");

        let cat_filter = category != "all";

        let fts_sql = if cat_filter {
            "SELECT topic_id, title, snippet(topics, 2, '>>', '<<', '...', 30) AS excerpt \
             FROM topics WHERE topics MATCH ?1 AND category = ?3 \
             ORDER BY rank LIMIT ?2"
        } else {
            "SELECT topic_id, title, snippet(topics, 2, '>>', '<<', '...', 30) AS excerpt \
             FROM topics WHERE topics MATCH ?1 \
             ORDER BY rank LIMIT ?2"
        };

        if let Ok(mut stmt) = self.conn.prepare(fts_sql) {
            let results: Vec<SearchResult> = if cat_filter {
                match stmt.query_map(params![fts_query, limit, category], |row| {
                    Ok(SearchResult { topic_id: row.get(0)?, title: row.get(1)?, excerpt: row.get(2)? })
                }) {
                    Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                    Err(_) => return self.fallback_search(query, category, limit),
                }
            } else {
                match stmt.query_map(params![fts_query, limit], |row| {
                    Ok(SearchResult { topic_id: row.get(0)?, title: row.get(1)?, excerpt: row.get(2)? })
                }) {
                    Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                    Err(_) => return self.fallback_search(query, category, limit),
                }
            };
            if !results.is_empty() {
                return Ok(results);
            }
        }

        self.fallback_search(query, category, limit)
    }

    fn fallback_search(&self, query: &str, category: &str, limit: u32) -> Result<Vec<SearchResult>, String> {
        let pattern = format!("%{}%", query);

        if category == "all" {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT topic_id, title, SUBSTR(content, 1, 120) AS excerpt
                     FROM topics WHERE title LIKE ?1 OR content LIKE ?1
                     LIMIT ?2",
                )
                .map_err(|e| format!("Fallback prepare error: {e}"))?;
            let rows = stmt
                .query_map(params![pattern, limit], |row| {
                    Ok(SearchResult {
                        topic_id: row.get(0)?,
                        title: row.get(1)?,
                        excerpt: row.get(2)?,
                    })
                })
                .map_err(|e| format!("Fallback query error: {e}"))?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        } else {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT topic_id, title, SUBSTR(content, 1, 120) AS excerpt
                     FROM topics WHERE (title LIKE ?1 OR content LIKE ?1) AND category = ?3
                     LIMIT ?2",
                )
                .map_err(|e| format!("Fallback prepare error: {e}"))?;
            let rows = stmt
                .query_map(params![pattern, limit, category], |row| {
                    Ok(SearchResult {
                        topic_id: row.get(0)?,
                        title: row.get(1)?,
                        excerpt: row.get(2)?,
                    })
                })
                .map_err(|e| format!("Fallback query error: {e}"))?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        }
    }

    pub fn get_topic(&self, topic_id: &str) -> Result<Option<TopicRow>, String> {
        self.conn
            .query_row(
                "SELECT topic_id, title, content, category, version
                 FROM topics WHERE topic_id = ?1",
                params![topic_id],
                |row| {
                    Ok(TopicRow {
                        topic_id: row.get(0)?,
                        title: row.get(1)?,
                        content: row.get(2)?,
                        category: row.get(3)?,
                        version: row.get(4)?,
                    })
                },
            )
            .map(Some)
            .or_else(|e| {
                if e == rusqlite::Error::QueryReturnedNoRows {
                    Ok(None)
                } else {
                    Err(format!("Failed to get topic: {e}"))
                }
            })
    }

    pub fn get_info(&self) -> Result<(Option<String>, u32, Option<String>), String> {
        let version = self.get_meta("version").ok().flatten();
        let indexed_at = self.get_meta("indexed_at").ok().flatten();
        let count = self
            .conn
            .query_row("SELECT COUNT(*) FROM topics", [], |row| row.get(0))
            .map_err(|e| format!("Failed to count topics: {e}"))?;
        Ok((version, count, indexed_at))
    }

    pub fn insert_topics(&self, topics: &[TopicRow]) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("Failed to start transaction: {e}"))?;

        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO topics (topic_id, title, content, category, version)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(|e| format!("Failed to prepare insert: {e}"))?;

            for topic in topics {
                stmt.execute(params![
                    topic.topic_id,
                    topic.title,
                    topic.content,
                    topic.category,
                    topic.version,
                ])
                .map_err(|e| format!("Failed to insert topic '{}': {e}", topic.topic_id))?;
            }
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {e}"))?;
        Ok(())
    }

    pub fn delete_version(&self, version: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM topics WHERE version = ?1",
                params![version],
            )
            .map_err(|e| format!("Failed to delete version {version}: {e}"))?;
        Ok(())
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(|e| format!("Failed to set meta '{key}': {e}"))?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>, String> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |row| {
                row.get(0)
            })
            .map(Some)
            .or_else(|e| {
                if e == rusqlite::Error::QueryReturnedNoRows {
                    Ok(None)
                } else {
                    Err(format!("Failed to get meta '{key}': {e}"))
                }
            })
    }

    pub fn topic_count(&self, version: &str) -> Result<u32, String> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM topics WHERE version = ?1",
                params![version],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to count topics for version {version}: {e}"))
    }
}
