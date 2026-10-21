//! Search: instant full-text matching plus an advanced syntax, compiled to SQL.
//!
//! ```text
//! radiohead                      words match any text field (title, artists, album, composer...)
//! "paranoid android"             a phrase
//! artist:"Radiohead"             a field contains the text   (artist, albumartist, album, title,
//! artist:=Radiohead              a field equals the text      composer, genre, path)
//! year:>2000  year:1990..1999    numbers: > >= < <= = and ranges (year, rating, plays, bits,
//!                                rate, channels, track, disc, duration, added)
//! format:flac  quality:hires     codec/container, or lossy | lossless | hires | dsd
//! rating:>=4                     ratings ≤ 5 are stars, otherwise 0–100
//! added:<30                      added in the last 30 days
//! fav:yes                        favourites
//! -live  -genre:podcast          exclude
//! ```
//!
//! Text matching uses an FTS5 word index with prefix indexes: every typed word matches words
//! that start with it ("radio" finds "Radiohead"), case- and accent-insensitively ("bjork" finds
//! "Björk"). Mid-word substrings are not matched: a trigram index can, but at 200 000 tracks it
//! is 3–5× slower and misses the 50 ms budget (measured; ARCHITECTURE D23).

use rusqlite::types::Value;

#[derive(Debug, Clone, PartialEq)]
enum Op {
    Contains,
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
    Range(f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
struct Clause {
    negate: bool,
    field: Option<String>,
    op: Op,
    value: String,
}

fn tokenize(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in q.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn unquote(s: &str) -> String {
    s.strip_prefix('"')
        .map(|r| r.strip_suffix('"').unwrap_or(r))
        .unwrap_or(s)
        .to_owned()
}

const FIELDS: &[&str] = &[
    "artist",
    "albumartist",
    "album",
    "title",
    "composer",
    "genre",
    "path",
    "year",
    "rating",
    "plays",
    "bits",
    "bitdepth",
    "rate",
    "samplerate",
    "channels",
    "track",
    "disc",
    "duration",
    "added",
    "format",
    "codec",
    "quality",
    "fav",
    "favourite",
    "favorite",
];

fn parse(q: &str) -> Vec<Clause> {
    tokenize(q)
        .into_iter()
        .filter_map(|tok| {
            let (negate, tok) = match tok.strip_prefix('-') {
                Some(rest) if !rest.is_empty() => (true, rest.to_owned()),
                _ => (false, tok),
            };
            if let Some((field, rest)) = tok.split_once(':') {
                let field = field.to_ascii_lowercase();
                if FIELDS.contains(&field.as_str()) && !rest.is_empty() {
                    let (op, value) = if let Some(v) = rest.strip_prefix(">=") {
                        (Op::Ge, v)
                    } else if let Some(v) = rest.strip_prefix("<=") {
                        (Op::Le, v)
                    } else if let Some(v) = rest.strip_prefix('>') {
                        (Op::Gt, v)
                    } else if let Some(v) = rest.strip_prefix('<') {
                        (Op::Lt, v)
                    } else if let Some(v) = rest.strip_prefix('=') {
                        (Op::Eq, v)
                    } else if let Some((a, b)) = rest.split_once("..") {
                        match (a.parse(), b.parse()) {
                            (Ok(a), Ok(b)) => (Op::Range(a, b), rest),
                            _ => (Op::Contains, rest),
                        }
                    } else {
                        (Op::Contains, rest)
                    };
                    return Some(Clause {
                        negate,
                        field: Some(field),
                        op,
                        value: unquote(value),
                    });
                }
            }
            let value = unquote(&tok);
            (!value.is_empty()).then_some(Clause {
                negate,
                field: None,
                op: Op::Contains,
                value,
            })
        })
        .collect()
}

/// A compiled query: SQL `WHERE` fragment (over `tracks t` joined with `user_data u`), its
/// parameters, and whether results can be ranked by the full-text index.
#[derive(Debug, Clone)]
pub(crate) struct Compiled {
    pub where_sql: String,
    pub params: Vec<Value>,
    pub fts_match: Option<String>,
}

/// A quoted FTS5 phrase whose last word is a prefix: `"paranoid andr"*`.
fn fts_quote(s: &str) -> String {
    format!("\"{}\"*", s.replace('"', "\"\""))
}

fn like(s: &str) -> String {
    format!(
        "%{}%",
        s.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

const TEXT_COLUMNS: &[(&str, &str)] = &[
    ("artist", "artist"),
    ("albumartist", "albumartist"),
    ("album", "album"),
    ("title", "title"),
    ("composer", "composer"),
    ("genre", "genre"),
];

pub(crate) fn compile(query: &str, now: i64) -> Compiled {
    let mut conds: Vec<String> = Vec::new();
    let mut params: Vec<Value> = Vec::new();
    let mut fts_terms: Vec<String> = Vec::new();
    // Negative full-text terms: folded into MATCH as `a NOT b` when there is a positive term
    // (one index lookup), otherwise evaluated as NOT IN (FTS5 has no unary NOT).
    let mut fts_not: Vec<String> = Vec::new();

    for c in parse(query) {
        let neg = if c.negate { "NOT " } else { "" };
        let field = c.field.as_deref();
        let num = c.value.parse::<f64>().ok();
        let numeric =
            |column: &str, scale: f64, conds: &mut Vec<String>, params: &mut Vec<Value>| {
                let Some(v) = num.or(match c.op {
                    Op::Range(..) => Some(0.0),
                    _ => None,
                }) else {
                    return;
                };
                let expr = match &c.op {
                    Op::Gt => format!("{column} > ?"),
                    Op::Ge => format!("{column} >= ?"),
                    Op::Lt => format!("{column} < ?"),
                    Op::Le => format!("{column} <= ?"),
                    Op::Eq | Op::Contains => format!("{column} = ?"),
                    Op::Range(a, b) => {
                        params.push(Value::Real(a * scale));
                        params.push(Value::Real(b * scale));
                        conds.push(format!("{neg}({column} BETWEEN ? AND ?)"));
                        return;
                    }
                };
                params.push(Value::Real(v * scale));
                // Plain comparisons keep the column indexable (NULL never matches). A negation must
                // count NULL as "not matching", hence the COALESCE there only.
                if c.negate {
                    conds.push(format!("NOT COALESCE({expr}, 0)"));
                } else {
                    conds.push(expr);
                }
            };
        match field {
            None => {
                if !c.value.chars().any(char::is_alphanumeric) {
                    continue; // punctuation only: nothing to match
                }
                if c.negate {
                    fts_not.push(fts_quote(&c.value));
                } else {
                    fts_terms.push(fts_quote(&c.value));
                }
            }
            Some(f) if TEXT_COLUMNS.iter().any(|(k, _)| *k == f) => {
                let col = TEXT_COLUMNS
                    .iter()
                    .find(|(k, _)| *k == f)
                    .map(|(_, c)| *c)
                    .unwrap_or("title");
                if c.op == Op::Eq {
                    conds.push(format!("{neg}(t.{col} = ? COLLATE NOCASE)"));
                    params.push(Value::Text(c.value.clone()));
                } else if c.value.chars().any(char::is_alphanumeric) {
                    let m = format!("{col} : {}", fts_quote(&c.value));
                    if c.negate {
                        fts_not.push(m);
                    } else {
                        fts_terms.push(m);
                    }
                }
            }
            Some("path") => {
                conds.push(format!("{neg}(t.rel_path LIKE ? ESCAPE '\\')"));
                params.push(Value::Text(like(&c.value)));
            }
            Some("year") => numeric("t.year", 1.0, &mut conds, &mut params),
            Some("rating") => {
                // ≤ 5 means stars (half stars allowed), otherwise the 0–100 scale.
                let scale = if num.is_some_and(|v| v <= 5.0)
                    || matches!(c.op, Op::Range(_, b) if b <= 5.0)
                {
                    20.0
                } else {
                    1.0
                };
                numeric("u.rating", scale, &mut conds, &mut params)
            }
            Some("plays") => numeric("u.play_count", 1.0, &mut conds, &mut params),
            Some("bits" | "bitdepth") => numeric("t.bit_depth", 1.0, &mut conds, &mut params),
            Some("rate" | "samplerate") => {
                let scale = if num.is_some_and(|v| v < 1000.0) {
                    1000.0
                } else {
                    1.0
                };
                numeric("t.sample_rate", scale, &mut conds, &mut params)
            }
            Some("channels") => numeric("t.channels", 1.0, &mut conds, &mut params),
            Some("track") => numeric("t.track_no", 1.0, &mut conds, &mut params),
            Some("disc") => numeric("t.disc_no", 1.0, &mut conds, &mut params),
            Some("duration") => numeric("t.duration_ms", 1000.0, &mut conds, &mut params),
            Some("added") => {
                // added:<30 → within the last 30 days: compare ages.
                numeric(
                    &format!("(({now} - t.added_at) / 86400.0)"),
                    1.0,
                    &mut conds,
                    &mut params,
                )
            }
            Some("quality") | Some("format") | Some("codec")
                if matches!(
                    c.value.to_ascii_lowercase().as_str(),
                    "lossy" | "lossless" | "hires" | "hi-res" | "dsd"
                ) =>
            {
                let v = c.value.to_ascii_lowercase().replace("hi-res", "hires");
                if v == "lossless" {
                    conds.push(format!("{neg}(t.lossless = 1)"));
                } else {
                    conds.push(format!("{neg}(t.quality = ?)"));
                    params.push(Value::Text(v));
                }
            }
            Some("quality" | "format" | "codec") => {
                let v = match c.value.to_ascii_lowercase().as_str() {
                    "wv" => "wavpack".to_owned(),
                    "m4a" | "mp4" => "mp4".to_owned(),
                    "ogg" => "vorbis".to_owned(),
                    other => other.to_owned(),
                };
                conds.push(format!("{neg}(t.codec = ? OR t.container = ?)"));
                params.push(Value::Text(v.clone()));
                params.push(Value::Text(v));
            }
            Some("fav" | "favourite" | "favorite") => {
                let yes = matches!(
                    c.value.to_ascii_lowercase().as_str(),
                    "yes" | "true" | "1" | "y"
                );
                conds.push(format!(
                    "{neg}(COALESCE(u.favourite, 0) = {})",
                    i32::from(yes)
                ));
            }
            Some(_) => {}
        }
    }
    if fts_terms.is_empty() {
        for m in fts_not.drain(..) {
            conds.push("t.id NOT IN (SELECT rowid FROM search WHERE search MATCH ?)".into());
            params.push(Value::Text(m));
        }
    }
    let fts_match = (!fts_terms.is_empty()).then(|| {
        let positive = fts_terms.join(" AND ");
        fts_not
            .iter()
            .fold(format!("({positive})"), |acc, n| format!("{acc} NOT {n}"))
    });
    let mut where_sql = String::from("t.missing_since IS NULL");
    for cond in conds {
        where_sql.push_str(" AND ");
        where_sql.push_str(&cond);
    }
    Compiled {
        where_sql,
        params,
        fts_match,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_spec_example() {
        let c = parse(r#"artist:"Radiohead" year:>2000 format:flac rating:>=4"#);
        assert_eq!(c.len(), 4);
        assert_eq!(c[0].field.as_deref(), Some("artist"));
        assert_eq!(c[0].value, "Radiohead");
        assert_eq!(c[1].op, Op::Gt);
        assert_eq!(c[3].op, Op::Ge);
    }

    #[test]
    fn negation_phrases_and_unknown_fields() {
        let c = parse(r#"-live "paranoid android" foo:bar"#);
        assert!(c[0].negate);
        assert_eq!(c[1].value, "paranoid android");
        assert_eq!(c[2].field, None);
        assert_eq!(c[2].value, "foo:bar");
    }

    #[test]
    fn compiles_fts_and_sql_parts() {
        let c = compile("radio artist:head year:1990..1999 -genre:live q", 0);
        assert_eq!(
            c.fts_match.as_deref(),
            Some(r#"("radio"* AND artist : "head"* AND "q"*) NOT genre : "live"*"#)
        );
        assert!(c.where_sql.contains("BETWEEN"));
        let c = compile("year:>2000", 0);
        assert!(
            c.where_sql.ends_with("t.year > ?"),
            "indexable: {}",
            c.where_sql
        );
        let c = compile("-live year:2000", 0);
        assert!(c.fts_match.is_none());
        assert!(c.where_sql.contains("NOT IN"));
    }
}
