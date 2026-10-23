//! Rediscover (D59): mixes and albums picked from the owner's own library and listening, on this
//! device only. Each pick is fixed for its day (or week) and changes with the next, so Home is
//! stable while you use it but has something new each morning.
//!
//! - **Album of the Day**: an album not played for three months (or never), unplayed ones
//!   favoured.
//! - **Still Unplayed** (weekly): albums added a while ago that have never been played through.
//! - **Forgotten Favourites**: favourites and much-played songs not heard for weeks.
//! - **Deep Cuts**: never-played songs from albums you do play.
//! - **On This Day**: what you played around today's date in earlier years.
//! - **Your Mornings / Afternoons / Evenings / Late Nights**: what you play at this time of day.
//!
//! Picking is a pure function of the gathered rows and the time (`build`), so it is tested
//! without a database; `gather` only reads.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use serde::Serialize;

use crate::{ALBUM_COLUMNS, AlbumRow, Library, Result, album_row};

const DAY: i64 = 86_400;
/// Songs in a mix.
const MIX_SIZE: usize = 25;
/// A mix shorter than this isn't worth showing.
const MIX_MIN: usize = 6;
/// Albums in the weekly Still Unplayed shelf.
const UNPLAYED_SIZE: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mix {
    /// Stable for the mix's kind (`forgotten`, `deep-cuts`, `on-this-day`, `daypart`).
    pub id: String,
    pub title: String,
    pub subtitle: String,
    /// `daily` or `weekly`.
    pub refresh: String,
    pub track_ids: Vec<i64>,
    /// Up to four different artworks from the mix, for its cover.
    pub art: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rediscover {
    pub album_of_the_day: Option<AlbumRow>,
    pub unplayed: Vec<AlbumRow>,
    pub mixes: Vec<Mix>,
}

/// One present track, as picking needs it.
#[derive(Debug, Clone)]
pub(crate) struct Song {
    pub id: i64,
    pub uid: String,
    pub album_id: Option<i64>,
    pub art: Option<String>,
    pub favourite: bool,
    pub plays: i64,
    pub last_played: Option<i64>,
}

/// Everything picking reads.
#[derive(Debug, Clone, Default)]
pub(crate) struct Gathered {
    pub songs: Vec<Song>,
    pub albums: Vec<AlbumRow>,
    /// (track uid, when), every listen.
    pub plays: Vec<(String, i64)>,
}

/// A number from 0 (exclusive) to 1, fixed for `key` and `seed` (FNV-1a, then the top 53 bits).
fn chance(key: &str, seed: i64) -> f64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes().chain(seed.to_le_bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // Mix the bits a little more so nearby seeds differ everywhere.
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    ((h >> 11) as f64 + 1.0) / (1u64 << 53) as f64
}

/// Up to `n` items, drawn without replacement with chances in proportion to their weights, the
/// same for the same seed (Efraimidis–Spirakis: the smallest −ln(u)/w win).
fn pick<T: Clone>(items: &[(T, &str, f64)], n: usize, seed: i64) -> Vec<T> {
    let mut keyed: Vec<(f64, &T)> = items
        .iter()
        .filter(|(_, _, w)| *w > 0.0)
        .map(|(t, key, w)| (-chance(key, seed).ln() / w, t))
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    keyed.into_iter().take(n).map(|(_, t)| t.clone()).collect()
}

/// The local day number (days since 1970-01-01 where the owner is).
fn local_day(at: i64, offset_min: i32) -> i64 {
    (at + i64::from(offset_min) * 60).div_euclid(DAY)
}

/// The local hour, 0–23.
fn local_hour(at: i64, offset_min: i32) -> i64 {
    (at + i64::from(offset_min) * 60).rem_euclid(DAY) / 3600
}

/// (year, month, day) of a day number (Howard Hinnant's civil_from_days).
fn civil(day: i64) -> (i64, i64, i64) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The day number of a date (days_from_civil).
fn day_of(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The part of the day an hour falls in, and its mix's title.
fn daypart(hour: i64) -> (usize, &'static str) {
    match hour {
        5..=11 => (0, "Your Mornings"),
        12..=16 => (1, "Your Afternoons"),
        17..=21 => (2, "Your Evenings"),
        _ => (3, "Late Nights"),
    }
}

/// Picks today's Rediscover from what was gathered. `now` in seconds; `offset_min` is the
/// owner's time zone (minutes east of UTC), so days and hours are theirs.
pub(crate) fn build(g: &Gathered, now: i64, offset_min: i32) -> Rediscover {
    let today = local_day(now, offset_min);
    // Weeks start on Monday: day 0 (1970-01-01) was a Thursday.
    let week = (today + 3).div_euclid(7);
    let by_uid: HashMap<&str, &Song> = g.songs.iter().map(|s| (s.uid.as_str(), s)).collect();
    let mut mixes = Vec::new();
    let mut mix = |id: &str, title: &str, subtitle: &str, ids: Vec<i64>| {
        if ids.len() < MIX_MIN {
            return;
        }
        let art_of: HashMap<i64, &str> = g
            .songs
            .iter()
            .filter_map(|s| s.art.as_deref().map(|a| (s.id, a)))
            .collect();
        let mut art: Vec<String> = Vec::new();
        for id in &ids {
            if let Some(a) = art_of.get(id)
                && !art.iter().any(|x| x == a)
            {
                art.push((*a).to_owned());
                if art.len() == 4 {
                    break;
                }
            }
        }
        mixes.push(Mix {
            id: id.to_owned(),
            title: title.to_owned(),
            subtitle: subtitle.to_owned(),
            refresh: "daily".to_owned(),
            track_ids: ids,
            art,
        });
    };

    // Forgotten Favourites: loved or played three times or more, not for 45 days.
    let forgotten: Vec<(i64, &str, f64)> = g
        .songs
        .iter()
        .filter(|s| {
            (s.favourite || s.plays >= 3) && s.last_played.is_some_and(|t| t < now - 45 * DAY)
        })
        .map(|s| {
            (
                s.id,
                s.uid.as_str(),
                s.plays as f64 + if s.favourite { 5.0 } else { 0.0 },
            )
        })
        .collect();
    mix(
        "forgotten",
        "Forgotten Favourites",
        "Songs you loved and haven't played in a while",
        pick(&forgotten, MIX_SIZE, today),
    );

    // Deep Cuts: never-played songs from albums you've played.
    let played_albums: HashSet<i64> = g
        .songs
        .iter()
        .filter(|s| s.plays > 0)
        .filter_map(|s| s.album_id)
        .collect();
    let deep: Vec<(i64, &str, f64)> = g
        .songs
        .iter()
        .filter(|s| s.plays == 0 && s.album_id.is_some_and(|a| played_albums.contains(&a)))
        .map(|s| (s.id, s.uid.as_str(), 1.0))
        .collect();
    mix(
        "deep-cuts",
        "Deep Cuts",
        "Songs you've never played, from albums you do",
        pick(&deep, MIX_SIZE, today + 1),
    );

    // On This Day: plays within three days of today's date in earlier years.
    let (ty, _, _) = civil(today);
    let mut then: HashMap<&str, f64> = HashMap::new();
    for (uid, at) in &g.plays {
        let day = local_day(*at, offset_min);
        if day > today - 300 {
            continue;
        }
        let (_, m, d) = civil(day);
        // The same date this year (29 February falls on 1 March in other years).
        let same = day_of(ty, m, d);
        if (same - today).abs() <= 3 && by_uid.contains_key(uid.as_str()) {
            *then.entry(uid).or_default() += 1.0;
        }
    }
    let on_this_day: Vec<(i64, &str, f64)> = then
        .iter()
        .map(|(uid, w)| (by_uid[uid].id, *uid, *w))
        .collect();
    mix(
        "on-this-day",
        "On This Day",
        "What you were playing around now in other years",
        pick(&on_this_day, MIX_SIZE, today + 2),
    );

    // This time of day: plays in the same part of the day over the last four months.
    let (part, title) = daypart(local_hour(now, offset_min));
    let mut usual: HashMap<&str, f64> = HashMap::new();
    for (uid, at) in &g.plays {
        if *at >= now - 120 * DAY
            && daypart(local_hour(*at, offset_min)).0 == part
            && by_uid.contains_key(uid.as_str())
        {
            *usual.entry(uid).or_default() += 1.0;
        }
    }
    let usual: Vec<(i64, &str, f64)> = usual
        .iter()
        .map(|(uid, w)| (by_uid[uid].id, *uid, *w))
        .collect();
    mix(
        "daypart",
        title,
        "What you tend to play at this time of day",
        pick(&usual, MIX_SIZE, today + 3),
    );

    // Albums: when each was last played, and whether ever.
    let mut last: HashMap<i64, i64> = HashMap::new();
    let mut ever: HashSet<i64> = HashSet::new();
    for s in &g.songs {
        if let Some(a) = s.album_id {
            if s.plays > 0 {
                ever.insert(a);
            }
            if let Some(t) = s.last_played {
                let e = last.entry(a).or_insert(t);
                *e = (*e).max(t);
            }
        }
    }
    let keys: Vec<String> = g.albums.iter().map(|a| format!("album:{}", a.id)).collect();
    let resting: Vec<(AlbumRow, &str, f64)> = g
        .albums
        .iter()
        .zip(&keys)
        .filter(|(a, _)| last.get(&a.id).is_none_or(|t| *t < now - 90 * DAY))
        .map(|(a, k)| {
            (
                a.clone(),
                k.as_str(),
                if ever.contains(&a.id) { 1.0 } else { 3.0 },
            )
        })
        .collect();
    let album_of_the_day = pick(&resting, 1, today).into_iter().next();
    let unplayed: Vec<(AlbumRow, &str, f64)> = g
        .albums
        .iter()
        .zip(&keys)
        .filter(|(a, _)| !ever.contains(&a.id) && a.added_at < now - 14 * DAY)
        .filter(|(a, _)| album_of_the_day.as_ref().is_none_or(|d| d.id != a.id))
        .map(|(a, k)| (a.clone(), k.as_str(), 1.0))
        .collect();
    Rediscover {
        album_of_the_day,
        unplayed: pick(&unplayed, UNPLAYED_SIZE, week),
        mixes,
    }
}

fn gather(c: &Connection) -> Result<Gathered> {
    let mut s = c.prepare_cached(
        "SELECT t.id, t.uid, t.album_id, t.art_hash, COALESCE(u.favourite, 0),
                COALESCE(u.play_count, 0), u.last_played
         FROM tracks t LEFT JOIN user_data u ON u.track_uid = t.uid
         WHERE t.missing_since IS NULL",
    )?;
    let songs = s
        .query_map([], |r| {
            Ok(Song {
                id: r.get(0)?,
                uid: r.get(1)?,
                album_id: r.get(2)?,
                art: r.get(3)?,
                favourite: r.get::<_, i64>(4)? != 0,
                plays: r.get(5)?,
                last_played: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut s = c.prepare_cached(&format!("SELECT {ALBUM_COLUMNS} FROM albums a"))?;
    let albums = s
        .query_map([], album_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut s = c.prepare_cached("SELECT track_uid, at FROM play_events")?;
    let plays = s
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Gathered {
        songs,
        albums,
        plays,
    })
}

impl Library {
    /// Today's Rediscover; `offset_min` is the owner's time zone in minutes east of UTC.
    pub fn rediscover(&self, now: i64, offset_min: i32) -> Result<Rediscover> {
        let g = self.db.read(gather)?;
        Ok(build(&g, now, offset_min))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_792_728_000; // 2026-10-23, a Friday, 04:00 UTC

    fn song(id: i64, album: i64, plays: i64, last: Option<i64>, fav: bool) -> Song {
        Song {
            id,
            uid: format!("u{id}"),
            album_id: Some(album),
            art: Some(format!("art{album}")),
            favourite: fav,
            plays,
            last_played: last,
        }
    }

    fn album(id: i64, added_at: i64) -> AlbumRow {
        AlbumRow {
            id,
            title: format!("Album {id}"),
            albumartist: "Someone".into(),
            year: None,
            genre: None,
            track_count: 10,
            duration_ms: 0,
            quality: None,
            art_hash: Some(format!("art{id}")),
            compilation: false,
            added_at,
        }
    }

    fn mix<'a>(r: &'a Rediscover, id: &str) -> Option<&'a Mix> {
        r.mixes.iter().find(|m| m.id == id)
    }

    #[test]
    fn dates_round_trip() {
        for day in [-1000, 0, 59, 60, 11_016, 20_384, 20_750] {
            let (y, m, d) = civil(day);
            assert_eq!(day_of(y, m, d), day);
        }
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(local_day(NOW, 0)), (2026, 10, 23));
    }

    #[test]
    fn picks_are_weighted_fixed_for_the_seed_and_change_with_it() {
        let items: Vec<(i64, String, f64)> = (0..200)
            .map(|i| (i, format!("k{i}"), if i < 10 { 50.0 } else { 1.0 }))
            .collect();
        let items: Vec<(i64, &str, f64)> =
            items.iter().map(|(i, k, w)| (*i, k.as_str(), *w)).collect();
        let a = pick(&items, 10, 1);
        assert_eq!(a, pick(&items, 10, 1));
        assert_ne!(a, pick(&items, 10, 2));
        // The heavy ten dominate.
        assert!(a.iter().filter(|i| **i < 10).count() >= 6, "{a:?}");
        let set: HashSet<_> = a.iter().collect();
        assert_eq!(set.len(), 10, "no repeats");
    }

    #[test]
    fn forgotten_favourites_are_loved_or_played_and_not_recent() {
        let mut g = Gathered::default();
        for i in 0..10 {
            g.songs.push(song(i, 1, 5, Some(NOW - 100 * DAY), false)); // forgotten
        }
        g.songs.push(song(20, 1, 0, Some(NOW - 100 * DAY), true)); // a forgotten favourite
        g.songs.push(song(21, 1, 9, Some(NOW - 2 * DAY), true)); // played lately
        g.songs.push(song(22, 1, 1, Some(NOW - 100 * DAY), false)); // played once
        let r = build(&g, NOW, 0);
        let m = mix(&r, "forgotten").expect("a mix");
        assert!(m.track_ids.contains(&20));
        assert!(!m.track_ids.contains(&21) && !m.track_ids.contains(&22));
        assert_eq!(m.track_ids.len(), 11);
        assert_eq!(m.art, vec!["art1".to_owned()]);
    }

    #[test]
    fn small_mixes_are_left_out() {
        let mut g = Gathered::default();
        for i in 0..3 {
            g.songs.push(song(i, 1, 5, Some(NOW - 100 * DAY), false));
        }
        assert!(mix(&build(&g, NOW, 0), "forgotten").is_none());
    }

    #[test]
    fn deep_cuts_come_from_albums_you_play() {
        let mut g = Gathered::default();
        g.songs.push(song(1, 1, 4, Some(NOW - DAY), false));
        for i in 2..12 {
            g.songs.push(song(i, 1, 0, None, false));
        }
        for i in 20..30 {
            g.songs.push(song(i, 2, 0, None, false)); // an album never played
        }
        let m = build(&g, NOW, 0);
        let m = mix(&m, "deep-cuts").expect("a mix");
        assert!(
            m.track_ids.iter().all(|i| (2..12).contains(i)),
            "{:?}",
            m.track_ids
        );
    }

    #[test]
    fn on_this_day_looks_at_the_same_dates_in_earlier_years() {
        let mut g = Gathered::default();
        for i in 0..20 {
            g.songs.push(song(i, 1, 1, Some(NOW - 400 * DAY), false));
        }
        // 2025-10-21 (two days off, a year ago) for 0–9; 2025-08-01 for 10–19.
        let near = day_of(2025, 10, 21) * DAY + 3600 * 12;
        let far = day_of(2025, 8, 1) * DAY;
        for i in 0..10 {
            g.plays.push((format!("u{i}"), near));
        }
        for i in 10..20 {
            g.plays.push((format!("u{i}"), far));
        }
        let r = build(&g, NOW, 0);
        let m = mix(&r, "on-this-day").expect("a mix");
        assert!(m.track_ids.iter().all(|i| *i < 10), "{:?}", m.track_ids);
        assert_eq!(m.track_ids.len(), 10);
    }

    #[test]
    fn the_time_of_day_mix_follows_the_owners_clock() {
        let mut g = Gathered::default();
        for i in 0..20 {
            g.songs.push(song(i, 1, 1, Some(NOW - DAY), false));
        }
        // Played at 08:00 and at 20:00 local time (UTC+2), on recent days.
        for i in 0..10 {
            g.plays
                .push((format!("u{i}"), (local_day(NOW, 0) - 3) * DAY + 6 * 3600));
        }
        for i in 10..20 {
            g.plays
                .push((format!("u{i}"), (local_day(NOW, 0) - 3) * DAY + 18 * 3600));
        }
        // Now is 04:00 UTC: 06:00 at UTC+2 (a morning).
        let morning = build(&g, NOW, 120);
        let m = mix(&morning, "daypart").expect("a mix");
        assert_eq!(m.title, "Your Mornings");
        assert!(m.track_ids.iter().all(|i| *i < 10));
        // At UTC−5 the same 06:00 UTC plays were at 01:00, and now is 23:00: late nights.
        let night = build(&g, NOW, -300);
        let m = mix(&night, "daypart").expect("a mix");
        assert_eq!(m.title, "Late Nights");
        assert!(m.track_ids.iter().all(|i| *i < 10));
    }

    #[test]
    fn albums_rest_before_coming_back_and_unplayed_ones_wait_a_fortnight() {
        let mut g = Gathered::default();
        g.albums.push(album(1, NOW - 400 * DAY)); // played yesterday
        g.albums.push(album(2, NOW - 400 * DAY)); // played long ago
        g.albums.push(album(3, NOW - 400 * DAY)); // never played
        g.albums.push(album(4, NOW - 3 * DAY)); // new, never played
        g.songs.push(song(1, 1, 3, Some(NOW - DAY), false));
        g.songs.push(song(2, 2, 3, Some(NOW - 200 * DAY), false));
        g.songs.push(song(3, 3, 0, None, false));
        g.songs.push(song(4, 4, 0, None, false));
        let r = build(&g, NOW, 0);
        let day = r.album_of_the_day.as_ref().expect("an album").id;
        assert_ne!(day, 1, "not one played yesterday");
        let unplayed: Vec<i64> = r.unplayed.iter().map(|a| a.id).collect();
        assert!(!unplayed.contains(&1) && !unplayed.contains(&2) && !unplayed.contains(&4));
        assert!(!unplayed.contains(&day), "not twice");
        // Fixed for the day, and for the week.
        let again = build(&g, NOW + 3600, 0);
        assert_eq!(again.album_of_the_day.map(|a| a.id), Some(day));
    }
}
