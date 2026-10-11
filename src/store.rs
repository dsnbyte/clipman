//! SQLite-backed clipboard history. Images live as PNG files next to the DB.

use crate::config;
use crate::model::*;
use anyhow::Result;
use rusqlite::{Connection, params, params_from_iter, types::Value};
use std::path::PathBuf;
use std::time::Duration;

pub struct Store {
    conn: Connection,
    blobs: PathBuf,
}

impl Store {
    pub fn open() -> Result<Store> {
        let dir = config::data_dir();
        let blobs = dir.join("blobs");
        std::fs::create_dir_all(&blobs)?;
        let conn = Connection::open(dir.join("history.db"))?;
        conn.busy_timeout(Duration::from_secs(3))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS items (
                 id         INTEGER PRIMARY KEY,
                 kind       INTEGER NOT NULL,
                 hash       INTEGER NOT NULL UNIQUE,
                 content    TEXT,
                 search     TEXT NOT NULL,
                 preview    TEXT NOT NULL,
                 size       INTEGER NOT NULL,
                 width      INTEGER NOT NULL DEFAULT 0,
                 height     INTEGER NOT NULL DEFAULT 0,
                 pinned     INTEGER NOT NULL DEFAULT 0,
                 created_at INTEGER NOT NULL,
                 used_at    INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS items_order ON items (pinned DESC, used_at DESC);",
        )?;
        Ok(Store { conn, blobs })
    }

    pub fn image_path(&self, hash: u64) -> PathBuf {
        self.blobs.join(format!("{hash:016x}.png"))
    }

    pub fn thumb_path(&self, hash: u64) -> PathBuf {
        self.blobs.join(format!("{hash:016x}.thumb.png"))
    }

    pub fn find(&self, hash: u64) -> Option<i64> {
        self.conn
            .query_row("SELECT id FROM items WHERE hash = ?1", [hash as i64], |r| r.get(0))
            .ok()
    }

    pub fn touch(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE items SET used_at = ?1 WHERE id = ?2", params![now_secs(), id])?;
        Ok(())
    }

    /// Inserts a new item. `dims` is only meaningful for images.
    pub fn insert(&self, payload: &Payload, hash: u64, dims: (u32, u32)) -> Result<i64> {
        let now = now_secs();
        let (content, search, preview, size): (Option<String>, String, String, i64) = match payload {
            Payload::Text(t) => (
                Some(t.clone()),
                t.chars().take(4096).collect::<String>().to_lowercase(),
                text_preview(t),
                t.len() as i64,
            ),
            Payload::Files(f) => {
                let joined = join_paths(f);
                (
                    Some(joined.clone()),
                    joined.chars().take(4096).collect::<String>().to_lowercase(),
                    files_preview(f),
                    f.len() as i64,
                )
            }
            Payload::Image(b) => (
                None,
                "image".to_string(),
                format!("Image {}×{}", dims.0, dims.1),
                b.len() as i64,
            ),
        };
        self.conn.execute(
            "INSERT OR IGNORE INTO items
             (kind, hash, content, search, preview, size, width, height, pinned, created_at, used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?9)",
            params![
                payload.kind().to_i64(),
                hash as i64,
                content,
                search,
                preview,
                size,
                dims.0,
                dims.1,
                now
            ],
        )?;
        self.find(hash).ok_or_else(|| anyhow::anyhow!("insert failed"))
    }

    pub fn list(&self, filter: Filter, query: &str, limit: usize) -> Result<Vec<ItemMeta>> {
        let mut sql = String::from(
            "SELECT id, kind, preview, size, width, height, pinned, used_at, hash FROM items WHERE 1=1",
        );
        let mut args: Vec<Value> = Vec::new();
        match filter {
            Filter::All => {}
            Filter::Pinned => sql.push_str(" AND pinned = 1"),
            Filter::Text => sql.push_str(" AND kind = 0"),
            Filter::Images => sql.push_str(" AND kind = 1"),
            Filter::Files => sql.push_str(" AND kind = 2"),
        }
        let q = query.trim().to_lowercase();
        if !q.is_empty() {
            sql.push_str(" AND instr(search, ?) > 0");
            args.push(Value::Text(q));
        }
        sql.push_str(" ORDER BY pinned DESC, used_at DESC, id DESC LIMIT ?");
        args.push(Value::Integer(limit as i64));

        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params_from_iter(args), |r| {
            Ok(ItemMeta {
                id: r.get(0)?,
                kind: Kind::from_i64(r.get(1)?),
                preview: r.get(2)?,
                size: r.get(3)?,
                width: r.get(4)?,
                height: r.get(5)?,
                pinned: r.get::<_, i64>(6)? != 0,
                used_at: r.get(7)?,
                hash: r.get::<_, i64>(8)? as u64,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// Loads the full content of an item.
    pub fn payload(&self, id: i64) -> Result<Option<Payload>> {
        let row: Option<(i64, Option<String>, i64)> = self
            .conn
            .query_row("SELECT kind, content, hash FROM items WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .ok();
        Ok(match row {
            None => None,
            Some((kind, content, hash)) => match Kind::from_i64(kind) {
                Kind::Text => content.map(Payload::Text),
                Kind::Files => content.map(|c| {
                    Payload::Files(c.lines().map(PathBuf::from).collect())
                }),
                Kind::Image => std::fs::read(self.image_path(hash as u64))
                    .ok()
                    .map(Payload::Image),
            },
        })
    }

    pub fn toggle_pin(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE items SET pinned = 1 - pinned WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let hash: Option<(i64, i64)> = self
            .conn
            .query_row("SELECT kind, hash FROM items WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .ok();
        self.conn.execute("DELETE FROM items WHERE id = ?1", [id])?;
        if let Some((1, h)) = hash {
            self.remove_blobs(h as u64);
        }
        Ok(())
    }

    /// Removes every item that is not pinned.
    pub fn clear_unpinned(&self) -> Result<()> {
        self.delete_where("pinned = 0 ORDER BY used_at DESC", 0)
    }

    /// Keeps only the newest `max` unpinned items.
    pub fn trim(&self, max: usize) -> Result<()> {
        self.delete_where("pinned = 0 ORDER BY used_at DESC, id DESC", max)
    }

    /// Deletes rows matching `cond` (which must include ORDER BY), skipping the first `keep`.
    fn delete_where(&self, cond: &str, keep: usize) -> Result<()> {
        let sql = format!("SELECT id, kind, hash FROM items WHERE {cond} LIMIT -1 OFFSET {keep}");
        let victims: Vec<(i64, i64, i64)> = {
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.filter_map(Result::ok).collect()
        };
        for (id, kind, hash) in victims {
            self.conn.execute("DELETE FROM items WHERE id = ?1", [id])?;
            if kind == 1 {
                self.remove_blobs(hash as u64);
            }
        }
        Ok(())
    }

    fn remove_blobs(&self, hash: u64) {
        let _ = std::fs::remove_file(self.image_path(hash));
        let _ = std::fs::remove_file(self.thumb_path(hash));
    }
}
