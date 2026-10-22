//! Tag edit history: what each batch of edits changed, so it can be undone.

use rusqlite::params;
use serde::Serialize;

use crate::db::now;
use crate::{Library, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditBatch {
    pub id: i64,
    /// Unix seconds.
    pub at: i64,
    pub description: String,
    /// Tracks with recorded changes.
    pub tracks: i64,
    pub written: i64,
    pub failed: i64,
    /// The batch this one undid.
    pub undo_of: Option<i64>,
    /// Undone by a later batch.
    pub undone: bool,
    pub finished: bool,
}

/// One field of one track, before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditChange {
    /// The track now, if it is still in the library.
    pub track_id: Option<i64>,
    pub track_uid: String,
    pub field: String,
    pub old: Vec<String>,
    pub new: Vec<String>,
}

impl Library {
    pub fn begin_edit_batch(&self, description: &str, undo_of: Option<i64>) -> Result<i64> {
        let description = description.to_owned();
        self.db.write(move |tx| {
            tx.execute(
                "INSERT INTO edit_batches (at, description, undo_of) VALUES (?, ?, ?)",
                params![now(), description, undo_of],
            )?;
            Ok(tx.last_insert_rowid())
        })
    }

    /// Records the changes written to one track.
    pub fn record_edit(
        &self,
        batch: i64,
        track_id: i64,
        changes: Vec<(String, Vec<String>, Vec<String>)>,
    ) -> Result<()> {
        self.db.write(move |tx| {
            let uid: String = tx.query_row("SELECT uid FROM tracks WHERE id = ?", [track_id], |r| r.get(0))?;
            let mut insert = tx.prepare_cached(
                "INSERT INTO edit_changes (batch_id, track_uid, field, old, new) VALUES (?, ?, ?, ?, ?)",
            )?;
            for (field, old, new) in changes {
                insert.execute(params![batch, uid, field, json(&old), json(&new)])?;
            }
            Ok(())
        })
    }

    /// Closes a batch; an undo batch marks the batch it undid.
    pub fn finish_edit_batch(&self, batch: i64, written: i64, failed: i64) -> Result<()> {
        self.db.write(move |tx| {
            tx.execute(
                "UPDATE edit_batches SET written = ?, failed = ?, finished = 1 WHERE id = ?",
                params![written, failed, batch],
            )?;
            if written > 0 {
                tx.execute(
                    "UPDATE edit_batches SET undone = 1
                     WHERE id = (SELECT undo_of FROM edit_batches WHERE id = ?)",
                    [batch],
                )?;
            }
            Ok(())
        })
    }

    /// The most recent batches first.
    pub fn edit_batches(&self, limit: i64) -> Result<Vec<EditBatch>> {
        self.db.read(|c| {
            let mut s = c.prepare_cached(
                "SELECT b.id, b.at, b.description, b.written, b.failed, b.undo_of, b.undone, b.finished,
                        (SELECT COUNT(DISTINCT track_uid) FROM edit_changes WHERE batch_id = b.id)
                 FROM edit_batches b ORDER BY b.id DESC LIMIT ?",
            )?;
            let rows = s.query_map([limit], |r| {
                Ok(EditBatch {
                    id: r.get(0)?,
                    at: r.get(1)?,
                    description: r.get(2)?,
                    written: r.get(3)?,
                    failed: r.get(4)?,
                    undo_of: r.get(5)?,
                    undone: r.get(6)?,
                    finished: r.get(7)?,
                    tracks: r.get(8)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn edit_changes(&self, batch: i64) -> Result<Vec<EditChange>> {
        self.db.read(|c| {
            let mut s = c.prepare_cached(
                "SELECT (SELECT id FROM tracks WHERE uid = e.track_uid LIMIT 1), e.track_uid, e.field, e.old, e.new
                 FROM edit_changes e WHERE e.batch_id = ? ORDER BY e.rowid",
            )?;
            let rows = s.query_map([batch], |r| {
                Ok(EditChange {
                    track_id: r.get(0)?,
                    track_uid: r.get(1)?,
                    field: r.get(2)?,
                    old: parse(&r.get::<_, String>(3)?),
                    new: parse(&r.get::<_, String>(4)?),
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }
}

fn json(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".into())
}

fn parse(text: &str) -> Vec<String> {
    serde_json::from_str(text).unwrap_or_default()
}
