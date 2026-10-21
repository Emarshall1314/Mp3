//! Album identity and aggregates.
//!
//! Albums group by (album artist, title, release). The release is the MusicBrainz release id
//! when tagged, otherwise the folder the tracks live in, so two "Greatest Hits" by different
//! artists never merge, and two editions of one album in separate folders stay separate but are
//! linked through `identity_key` ("Other versions"). Disc sub-folders ("CD1", "Disc 2") count as
//! their parent folder.

use std::collections::HashSet;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::Result;
use crate::art::StoredArt;
use crate::db::now;
use crate::text::{identity, sort_key, year};

#[derive(Debug, Clone)]
pub(crate) struct AlbumIdentity {
    pub title: String,
    pub albumartist: String,
    pub release_mbid: Option<String>,
    pub folder: String,
}

fn is_disc_folder(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    let rest = ["cd", "disc", "disk", "dvd"]
        .iter()
        .find_map(|p| n.strip_prefix(p))
        .map(|r| r.trim_start_matches([' ', '_', '-', '.']));
    rest.is_some_and(|r| !r.is_empty() && r.chars().take_while(|c| c.is_ascii_digit()).count() > 0)
}

pub(crate) fn album_folder(folder: &str) -> &str {
    match folder.rsplit_once('/') {
        Some((parent, last)) if is_disc_folder(last) => parent,
        None if is_disc_folder(folder) => "",
        _ => folder,
    }
}

impl AlbumIdentity {
    pub(crate) fn identity_key(&self) -> String {
        format!(
            "{}\u{1f}{}",
            identity(&self.albumartist),
            identity(&self.title)
        )
    }

    pub(crate) fn key(&self) -> String {
        let release = match &self.release_mbid {
            Some(mbid) => format!("mb:{}", mbid.to_ascii_lowercase()),
            None => format!("dir:{}", album_folder(&self.folder)),
        };
        // Untitled tracks group per folder, never across the whole library.
        format!("{}\u{1f}{release}", self.identity_key())
    }
}

/// Finds or creates the album for `id`; returns its row id.
pub(crate) fn upsert(
    tx: &Transaction<'_>,
    id: &AlbumIdentity,
    fields: &cadenza_tags::Fields,
    art: Option<&StoredArt>,
) -> Result<i64> {
    let key = id.key();
    if let Some(album_id) = tx
        .query_row("SELECT id FROM albums WHERE key = ?", [&key], |r| r.get(0))
        .optional()?
    {
        return Ok(album_id);
    }
    let first = |k: &str| fields.get(k).and_then(|v| v.first()).cloned();
    tx.execute(
        "INSERT INTO albums (key, title, albumartist, sort_title, sort_artist, identity_key, release_mbid,
            release_group_mbid, folder, art_hash, added_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            key,
            id.title,
            id.albumartist,
            sort_key(&first("albumsort").unwrap_or_else(|| id.title.clone())),
            sort_key(&first("albumartistsort").unwrap_or_else(|| id.albumartist.clone())),
            id.identity_key(),
            id.release_mbid,
            first("musicbrainz_releasegroupid"),
            album_folder(&id.folder),
            art.map(|a| a.hash.clone()),
            now(),
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Recomputes aggregates for the given albums from their present tracks; drops empty ones.
pub(crate) fn refresh(tx: &Transaction<'_>, albums: &HashSet<i64>) -> Result<()> {
    for &id in albums {
        let row: (i64, i64, i64, Option<i64>, Option<i64>, i64) = tx.query_row(
            "SELECT COUNT(*), COALESCE(MAX(COALESCE(disc_no, 1)), 1), COALESCE(SUM(duration_ms), 0), MIN(year),
                    MIN(added_at), COALESCE(MAX(compilation), 0)
             FROM tracks WHERE album_id = ? AND missing_since IS NULL",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )?;
        if row.0 == 0 {
            tx.execute("UPDATE tracks SET album_id = NULL WHERE album_id = ? AND missing_since IS NOT NULL", [id])?;
            tx.execute("DELETE FROM albums WHERE id = ?", [id])?;
            continue;
        }
        // Best quality on the album: dsd > hires > lossless > lossy.
        let quality: Option<String> = tx.query_row(
            "SELECT quality FROM tracks WHERE album_id = ? AND missing_since IS NULL
             ORDER BY CASE quality WHEN 'dsd' THEN 0 WHEN 'hires' THEN 1 WHEN 'lossless' THEN 2 ELSE 3 END LIMIT 1",
            [id],
            |r| r.get(0),
        )?;
        let genre: Option<String> = tx.query_row(
            "SELECT genre FROM tracks WHERE album_id = ? AND missing_since IS NULL AND genre != ''
             GROUP BY genre ORDER BY COUNT(*) DESC LIMIT 1",
            [id],
            |r| r.get(0),
        ).optional()?;
        let art: Option<String> = tx.query_row(
            "SELECT art_hash FROM tracks WHERE album_id = ? AND missing_since IS NULL AND art_hash IS NOT NULL
             ORDER BY disc_no, track_no LIMIT 1",
            [id],
            |r| r.get(0),
        ).optional()?;
        let (date, label): (Option<String>, Option<String>) = tx
            .query_row(
                "SELECT (SELECT value FROM tags WHERE track_id = t.id AND field = 'date' LIMIT 1),
                        (SELECT value FROM tags WHERE track_id = t.id AND field = 'label' LIMIT 1)
                 FROM tracks t WHERE album_id = ? AND missing_since IS NULL ORDER BY disc_no, track_no LIMIT 1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((None, None));
        tx.execute(
            "UPDATE albums SET track_count = ?, disc_count = ?, duration_ms = ?, year = ?, added_at = COALESCE(?, added_at),
                compilation = ?, quality = ?, genre = ?, art_hash = COALESCE(?, art_hash), date = ?, label = ?
             WHERE id = ?",
            params![row.0, row.1, row.2, row.3.or(date.as_deref().and_then(year)), row.4, row.5, quality, genre, art, date, label, id],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_folders_collapse_into_the_album_folder() {
        assert_eq!(album_folder("Artist/Album/CD1"), "Artist/Album");
        assert_eq!(album_folder("Artist/Album/Disc 2"), "Artist/Album");
        assert_eq!(album_folder("Artist/Album"), "Artist/Album");
        assert_eq!(album_folder("Artist/Discography"), "Artist/Discography");
    }
}
