//! Composite pages: an artist's page and Home.

use rusqlite::{Connection, params};
use serde::Serialize;

use crate::{ALBUM_COLUMNS, AlbumRow, Library, Result, TRACK_COLUMNS, TRACK_FROM, TrackRow};
use crate::{album_row, track_row};

/// Release-type sections, in display order.
const SECTIONS: [&str; 7] = [
    "albums",
    "eps",
    "singles",
    "live",
    "compilations",
    "soundtracks",
    "other",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseSection {
    /// albums, eps, singles, live, compilations, soundtracks or other.
    pub kind: String,
    pub albums: Vec<AlbumRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistPage {
    pub name: String,
    /// Artwork of the artist's most recent release.
    pub art_hash: Option<String>,
    /// Most played songs (played at least once), up to ten.
    pub top_songs: Vec<TrackRow>,
    pub sections: Vec<ReleaseSection>,
    /// Other artists' albums with songs by this artist.
    pub appears_on: Vec<AlbumRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Home {
    pub recently_played: Vec<AlbumRow>,
    pub recently_added: Vec<AlbumRow>,
    pub most_played: Vec<AlbumRow>,
}

/// The section for a MusicBrainz release type ("album", "ep", "album; live", "Album/Soundtrack"…).
/// Secondary types (live, compilation, soundtrack) win over the primary one.
pub(crate) fn section(release_type: Option<&str>, compilation: bool) -> &'static str {
    let t = release_type.unwrap_or("").to_lowercase();
    let has = |w: &str| {
        t.split(|c: char| !c.is_alphanumeric())
            .any(|part| part == w)
    };
    if has("live") {
        "live"
    } else if has("soundtrack") {
        "soundtracks"
    } else if has("compilation") || (compilation && t.is_empty()) {
        "compilations"
    } else if has("ep") {
        "eps"
    } else if has("single") {
        "singles"
    } else if t.is_empty() || has("album") {
        "albums"
    } else {
        "other"
    }
}

fn albums(c: &Connection, sql: &str, p: impl rusqlite::Params) -> Result<Vec<AlbumRow>> {
    let mut s = c.prepare_cached(sql)?;
    let rows = s.query_map(p, album_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

impl Library {
    pub fn artist_page(&self, name: &str) -> Result<ArtistPage> {
        let name = name.to_owned();
        self.db.read(move |c| {
            let mut s = c.prepare_cached(&format!(
                "SELECT {ALBUM_COLUMNS},
                    (SELECT g.value FROM tracks t JOIN tags g ON g.track_id = t.id
                     WHERE t.album_id = a.id AND g.field = 'releasetype' LIMIT 1)
                 FROM albums a WHERE a.albumartist = ? COLLATE NOCASE
                 ORDER BY a.year DESC, a.sort_title"
            ))?;
            let own: Vec<(AlbumRow, Option<String>)> = s
                .query_map([&name], |r| Ok((album_row(r)?, r.get(11)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let art_hash = own.iter().find_map(|(a, _)| a.art_hash.clone());
            let sections = SECTIONS
                .iter()
                .map(|kind| ReleaseSection {
                    kind: (*kind).to_owned(),
                    albums: own
                        .iter()
                        .filter(|(a, t)| section(t.as_deref(), a.compilation) == *kind)
                        .map(|(a, _)| a.clone())
                        .collect(),
                })
                .filter(|s| !s.albums.is_empty())
                .collect();
            let mut s = c.prepare_cached(&format!(
                "SELECT {TRACK_COLUMNS} FROM {TRACK_FROM}
                 WHERE t.missing_since IS NULL AND u.play_count > 0
                   AND (t.albumartist = ?1 COLLATE NOCASE OR t.id IN
                        (SELECT track_id FROM tags WHERE field = 'artist' AND value = ?1 COLLATE NOCASE))
                 ORDER BY u.play_count DESC, u.last_played DESC LIMIT 10"
            ))?;
            let top_songs = s
                .query_map([&name], track_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let appears_on = albums(
                c,
                &format!(
                    "SELECT {ALBUM_COLUMNS} FROM albums a WHERE a.albumartist <> ?1 COLLATE NOCASE
                       AND a.id IN (SELECT t.album_id FROM tracks t JOIN tags g ON g.track_id = t.id
                                    WHERE g.field = 'artist' AND g.value = ?1 COLLATE NOCASE
                                      AND t.missing_since IS NULL)
                     ORDER BY a.year DESC, a.sort_title"
                ),
                [&name],
            )?;
            Ok(ArtistPage {
                name,
                art_hash,
                top_songs,
                sections,
                appears_on,
            })
        })
    }

    pub fn home(&self) -> Result<Home> {
        self.db.read(|c| {
            let per_album = |measure: &str, filter: &str| {
                format!(
                    "SELECT {ALBUM_COLUMNS} FROM albums a JOIN (
                        SELECT t.album_id, {measure} AS m FROM tracks t JOIN user_data u ON u.track_uid = t.uid
                        WHERE {filter} AND t.missing_since IS NULL GROUP BY t.album_id
                     ) p ON p.album_id = a.id ORDER BY p.m DESC LIMIT ?"
                )
            };
            Ok(Home {
                recently_played: albums(
                    c,
                    &per_album("MAX(u.last_played)", "u.last_played IS NOT NULL"),
                    params![12],
                )?,
                recently_added: albums(
                    c,
                    &format!("SELECT {ALBUM_COLUMNS} FROM albums a ORDER BY a.added_at DESC, a.id DESC LIMIT ?"),
                    params![12],
                )?,
                most_played: albums(
                    c,
                    &per_album("SUM(u.play_count)", "u.play_count > 0"),
                    params![12],
                )?,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::section;

    #[test]
    fn release_types_map_to_sections() {
        assert_eq!(section(Some("album"), false), "albums");
        assert_eq!(section(None, false), "albums");
        assert_eq!(section(None, true), "compilations");
        assert_eq!(section(Some("EP"), false), "eps");
        assert_eq!(section(Some("single"), false), "singles");
        assert_eq!(section(Some("album; live"), false), "live");
        assert_eq!(section(Some("Album/Soundtrack"), false), "soundtracks");
        assert_eq!(section(Some("album; compilation"), false), "compilations");
        assert_eq!(section(Some("broadcast"), false), "other");
    }
}
