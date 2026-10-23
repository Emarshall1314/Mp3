//! Playlists: named, ordered lists of songs (D61). Items name songs by track uid, so a playlist
//! survives files moving and rescans; a song that's gone (removed, or in the Trash) is skipped
//! when the playlist is read, and comes back if the song does. Items are ordered by fractional
//! position keys (`key_between`): adding, inserting or moving one never touches the others,
//! which is what lets two devices' edits merge when playlists sync.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::db::now;
use crate::{Error, Library, Result, TRACK_COLUMNS, TrackRow, track_row};

/// Digits of a position key, in sorting order.
const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn digit(c: u8) -> usize {
    DIGITS.iter().position(|d| *d == c).unwrap_or(0)
}

/// A key sorting strictly between `a` and `b` (both optional: `None` is the start or end).
/// Keys are base-36 fractions without trailing zeros; between two neighbours a key grows by at
/// most one digit.
pub(crate) fn key_between(a: Option<&str>, b: Option<&str>) -> String {
    fn mid(a: &[u8], b: Option<&[u8]>) -> Vec<u8> {
        if let Some(b) = b {
            // A shared prefix stays; look after it.
            let mut n = 0;
            while n < b.len() && a.get(n).copied().unwrap_or(b'0') == b[n] {
                n += 1;
            }
            if n > 0 {
                let mut out = b[..n].to_vec();
                out.extend(mid(a.get(n..).unwrap_or(&[]), Some(&b[n..])));
                return out;
            }
        }
        let da = a.first().map_or(0, |c| digit(*c));
        // At the end (appending, the common case), one step up rather than halfway, so a long
        // playlist's keys grow by a digit every 35 songs instead of every few.
        if b.is_none() && da + 1 < DIGITS.len() {
            return vec![DIGITS[da + 1]];
        }
        let db = b
            .and_then(|b| b.first())
            .map_or(DIGITS.len(), |c| digit(*c));
        if db - da > 1 {
            vec![DIGITS[(da + db) / 2]]
        } else if let Some(b) = b
            && b.len() > 1
        {
            // b's first digit alone sorts between a and b.
            vec![b[0]]
        } else {
            let mut out = vec![DIGITS[da]];
            out.extend(mid(a.get(1..).unwrap_or(&[]), None));
            out
        }
    }
    let out = mid(a.unwrap_or("").as_bytes(), b.map(str::as_bytes));
    String::from_utf8(out).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    /// Songs that are here (ones gone from the library aren't counted).
    pub track_count: i64,
    pub duration_ms: i64,
    /// Up to four different artworks, in playlist order, for its cover.
    pub art: Vec<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItem {
    /// The item, not the track: the same song can be in a playlist twice.
    pub item_id: i64,
    pub track: TrackRow,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDetail {
    pub playlist: Playlist,
    pub items: Vec<PlaylistItem>,
}

/// Songs that are here: in the library and not in the Trash.
const PRESENT: &str = "JOIN tracks t ON t.uid = i.track_uid AND t.missing_since IS NULL";

/// The present songs of a playlist, in order.
fn items(c: &Connection, id: i64) -> Result<Vec<PlaylistItem>> {
    // The track's columns first (track_row reads from 0), then the item's id.
    let mut s = c.prepare_cached(&format!(
        "SELECT {TRACK_COLUMNS}, i.id FROM playlist_items i {PRESENT}
         LEFT JOIN user_data u ON u.track_uid = t.uid
         WHERE i.playlist_id = ? ORDER BY i.pos, i.id"
    ))?;
    let rows = s.query_map([id], |r| {
        Ok(PlaylistItem {
            track: track_row(r)?,
            item_id: r.get(r.as_ref().column_count() - 1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn summary(c: &Connection, id: i64) -> Result<Playlist> {
    let (name, updated_at): (String, i64) = c
        .query_row(
            "SELECT name, updated_at FROM playlists WHERE id = ?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("playlist {id}")))?;
    let (track_count, duration_ms): (i64, i64) = c.query_row(
        &format!(
            "SELECT COUNT(*), COALESCE(SUM(t.duration_ms), 0) FROM playlist_items i {PRESENT}
             WHERE i.playlist_id = ?"
        ),
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let art = c
        .prepare_cached(&format!(
            "SELECT t.art_hash FROM playlist_items i {PRESENT}
             WHERE i.playlist_id = ? AND t.art_hash IS NOT NULL
             GROUP BY t.art_hash ORDER BY MIN(i.pos) LIMIT 4"
        ))?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(Playlist {
        id,
        name,
        track_count,
        duration_ms,
        art,
        updated_at,
    })
}

fn touch(c: &Connection, id: i64) -> Result<()> {
    let n = c.execute(
        "UPDATE playlists SET updated_at = ? WHERE id = ?",
        params![now(), id],
    )?;
    if n == 0 {
        return Err(Error::NotFound(format!("playlist {id}")));
    }
    Ok(())
}

/// Appends songs (by track id) to the end of a playlist.
fn append(c: &Connection, id: i64, track_ids: &[i64]) -> Result<()> {
    let mut last: Option<String> = c
        .query_row(
            "SELECT pos FROM playlist_items WHERE playlist_id = ? ORDER BY pos DESC LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let mut uid = c.prepare_cached("SELECT uid FROM tracks WHERE id = ?")?;
    let mut add = c.prepare_cached(
        "INSERT INTO playlist_items (playlist_id, track_uid, pos, added_at) VALUES (?, ?, ?, ?)",
    )?;
    let at = now();
    for t in track_ids {
        let Some(u): Option<String> = uid.query_row([t], |r| r.get(0)).optional()? else {
            continue;
        };
        let pos = key_between(last.as_deref(), None);
        add.execute(params![id, u, pos, at])?;
        last = Some(pos);
    }
    Ok(())
}

impl Library {
    /// Every playlist, most recently changed first.
    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        self.db.read(|c| {
            let ids: Vec<i64> = c
                .prepare_cached("SELECT id FROM playlists ORDER BY updated_at DESC, id DESC")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            ids.into_iter().map(|id| summary(c, id)).collect()
        })
    }

    pub fn playlist(&self, id: i64) -> Result<PlaylistDetail> {
        self.db.read(|c| {
            Ok(PlaylistDetail {
                playlist: summary(c, id)?,
                items: items(c, id)?,
            })
        })
    }

    /// A new playlist, with these songs in it (none for an empty one).
    pub fn create_playlist(&self, name: &str, track_ids: &[i64]) -> Result<Playlist> {
        let name = clean_name(name);
        let track_ids = track_ids.to_vec();
        let id = self.db.write(move |tx| {
            let at = now();
            tx.execute(
                "INSERT INTO playlists (uid, name, created_at, updated_at) VALUES (?, ?, ?, ?)",
                params![uuid::Uuid::now_v7().to_string(), name, at, at],
            )?;
            let id = tx.last_insert_rowid();
            append(tx, id, &track_ids)?;
            Ok(id)
        })?;
        self.db.read(|c| summary(c, id))
    }

    pub fn rename_playlist(&self, id: i64, name: &str) -> Result<()> {
        let name = clean_name(name);
        self.db.write(move |tx| {
            touch(tx, id)?;
            tx.execute(
                "UPDATE playlists SET name = ? WHERE id = ?",
                params![name, id],
            )?;
            Ok(())
        })
    }

    pub fn delete_playlist(&self, id: i64) -> Result<()> {
        self.db.write(move |tx| {
            tx.execute("DELETE FROM playlists WHERE id = ?", [id])?;
            Ok(())
        })
    }

    /// Adds songs to the end, in the order given (a song already there is added again).
    pub fn add_to_playlist(&self, id: i64, track_ids: &[i64]) -> Result<()> {
        let track_ids = track_ids.to_vec();
        self.db.write(move |tx| {
            touch(tx, id)?;
            append(tx, id, &track_ids)
        })
    }

    pub fn remove_from_playlist(&self, id: i64, item_ids: &[i64]) -> Result<()> {
        let item_ids = item_ids.to_vec();
        self.db.write(move |tx| {
            touch(tx, id)?;
            let mut s =
                tx.prepare_cached("DELETE FROM playlist_items WHERE playlist_id = ? AND id = ?")?;
            for item in item_ids {
                s.execute(params![id, item])?;
            }
            Ok(())
        })
    }

    /// Moves an item to just before `before` (another item of the same playlist), or to the
    /// end when `before` is `None`.
    pub fn move_in_playlist(&self, id: i64, item_id: i64, before: Option<i64>) -> Result<()> {
        self.db.write(move |tx| {
            touch(tx, id)?;
            let pos_of = |item: i64| -> Result<String> {
                tx.query_row(
                    "SELECT pos FROM playlist_items WHERE playlist_id = ? AND id = ?",
                    params![id, item],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| Error::NotFound(format!("playlist item {item}")))
            };
            pos_of(item_id)?;
            let (lo, hi) = match before {
                Some(b) if b == item_id => return Ok(()),
                Some(b) => {
                    let hi = pos_of(b)?;
                    let lo: Option<String> = tx
                        .query_row(
                            "SELECT pos FROM playlist_items WHERE playlist_id = ? AND pos < ? AND id != ?
                             ORDER BY pos DESC LIMIT 1",
                            params![id, hi, item_id],
                            |r| r.get(0),
                        )
                        .optional()?;
                    (lo, Some(hi))
                }
                None => {
                    let lo: Option<String> = tx
                        .query_row(
                            "SELECT pos FROM playlist_items WHERE playlist_id = ? AND id != ?
                             ORDER BY pos DESC LIMIT 1",
                            params![id, item_id],
                            |r| r.get(0),
                        )
                        .optional()?;
                    (lo, None)
                }
            };
            let pos = key_between(lo.as_deref(), hi.as_deref());
            tx.execute(
                "UPDATE playlist_items SET pos = ? WHERE id = ?",
                params![pos, item_id],
            )?;
            Ok(())
        })
    }
}

/// A name trimmed, never empty.
fn clean_name(name: &str) -> String {
    let n = name.trim();
    if n.is_empty() {
        "Untitled Playlist".to_owned()
    } else {
        n.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_sort_between_their_neighbours() {
        let first = key_between(None, None);
        let after = key_between(Some(&first), None);
        let before = key_between(None, Some(&first));
        assert!(before < first && first < after, "{before} {first} {after}");
        // Squeezing repeatedly into the same gap keeps working and stays short.
        let (mut lo, hi) = (first.clone(), after.clone());
        for _ in 0..200 {
            let k = key_between(Some(&lo), Some(&hi));
            assert!(lo < k && k < hi, "{lo} < {k} < {hi}");
            lo = k;
        }
        assert!(lo.len() < 60, "{}", lo.len());
        // Appending a thousand times grows slowly.
        let mut k = first;
        for _ in 0..1000 {
            let n = key_between(Some(&k), None);
            assert!(n > k);
            k = n;
        }
        assert!(k.len() < 40, "{}", k.len());
    }

    #[test]
    fn keys_never_end_in_zero() {
        let mut lo: Option<String> = None;
        for _ in 0..100 {
            let k = key_between(None, lo.as_deref());
            assert!(!k.ends_with('0'), "{k}");
            lo = Some(k);
        }
    }
}
