//! The play queue: items, current position, history, repeat and shuffle.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub track_id: i64,
    #[serde(skip)]
    pub path: PathBuf,
    /// CUE track range within the file, in its frames.
    #[serde(skip)]
    pub start_frame: Option<u64>,
    #[serde(skip)]
    pub end_frame: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

/// Items in play order. Shuffling reorders the upcoming part in place and remembers the
/// original order so turning shuffle off restores it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Queue {
    pub items: Vec<QueueItem>,
    pub current: Option<usize>,
    pub repeat: Repeat,
    pub shuffle: bool,
    unshuffled: Option<Vec<QueueItem>>,
    seed: u64,
}

impl Queue {
    pub fn set(&mut self, items: Vec<QueueItem>, start: usize) {
        self.items = items;
        self.unshuffled = None;
        self.current = (!self.items.is_empty()).then_some(start.min(self.items.len() - 1));
        if self.shuffle {
            self.shuffle_upcoming();
        }
    }

    pub fn current_item(&self) -> Option<&QueueItem> {
        self.current.and_then(|i| self.items.get(i))
    }

    /// The index after `i` in play order, honouring repeat (Repeat::One repeats `i`).
    pub fn after(&self, i: usize) -> Option<usize> {
        match self.repeat {
            Repeat::One => Some(i),
            _ if i + 1 < self.items.len() => Some(i + 1),
            Repeat::All if !self.items.is_empty() => Some(0),
            _ => None,
        }
    }

    pub fn before(&self, i: usize) -> Option<usize> {
        match (i, self.repeat) {
            (0, Repeat::All) if !self.items.is_empty() => Some(self.items.len() - 1),
            (0, _) => None,
            (i, _) => Some(i - 1),
        }
    }

    /// Play Next: right after the current item.
    pub fn insert_next(&mut self, items: Vec<QueueItem>) {
        let at = self.current.map(|c| c + 1).unwrap_or(0);
        self.items.splice(at..at, items.clone());
        if let Some(orig) = self.unshuffled.as_mut() {
            orig.extend(items);
        }
        if self.current.is_none() && !self.items.is_empty() {
            self.current = Some(0);
        }
    }

    /// Play Later: at the end.
    pub fn append(&mut self, items: Vec<QueueItem>) {
        self.items.extend(items.iter().cloned());
        if let Some(orig) = self.unshuffled.as_mut() {
            orig.extend(items);
        }
        if self.current.is_none() && !self.items.is_empty() {
            self.current = Some(0);
        }
    }

    /// Removes an item; returns true when it was the current one.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        let removed = self.items.remove(index);
        if let Some(orig) = self.unshuffled.as_mut()
            && let Some(p) = orig.iter().position(|i| i == &removed)
        {
            orig.remove(p);
        }
        match self.current {
            Some(c) if c == index => {
                if self.items.is_empty() {
                    self.current = None;
                } else {
                    self.current = Some(c.min(self.items.len() - 1));
                }
                true
            }
            Some(c) if c > index => {
                self.current = Some(c - 1);
                false
            }
            _ => false,
        }
    }

    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
        if let Some(c) = self.current {
            self.current = Some(if c == from {
                to
            } else if from < c && to >= c {
                c - 1
            } else if from > c && to <= c {
                c + 1
            } else {
                c
            });
        }
    }

    /// Drops everything after the current item.
    pub fn clear_upcoming(&mut self) {
        let keep = self.current.map(|c| c + 1).unwrap_or(0);
        self.items.truncate(keep);
        self.unshuffled = None;
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if on {
            self.shuffle_upcoming();
        } else if let Some(orig) = self.unshuffled.take() {
            let current = self.current_item().cloned();
            self.items = orig;
            self.current = current.and_then(|c| self.items.iter().position(|i| i == &c));
        }
    }

    fn shuffle_upcoming(&mut self) {
        self.unshuffled = Some(self.items.clone());
        let start = self.current.map(|c| c + 1).unwrap_or(0);
        // xorshift seeded per shuffle: no dependency, and deterministic per seed for tests.
        self.seed = self.seed.wrapping_add(0x9e37_79b9_7f4a_7c15) | 1;
        let mut x = self.seed;
        let upcoming = &mut self.items[start..];
        for i in (1..upcoming.len()).rev() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            upcoming.swap(i, (x % (i as u64 + 1)) as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(n: i64) -> Vec<QueueItem> {
        (0..n)
            .map(|i| QueueItem {
                track_id: i,
                path: PathBuf::new(),
                start_frame: None,
                end_frame: None,
            })
            .collect()
    }

    fn ids(q: &Queue) -> Vec<i64> {
        q.items.iter().map(|i| i.track_id).collect()
    }

    #[test]
    fn play_next_and_later() {
        let mut q = Queue::default();
        q.set(items(3), 1);
        q.insert_next(vec![QueueItem {
            track_id: 9,
            ..items(1)[0].clone()
        }]);
        q.append(vec![QueueItem {
            track_id: 8,
            ..items(1)[0].clone()
        }]);
        assert_eq!(ids(&q), [0, 1, 9, 2, 8]);
        assert_eq!(q.current, Some(1));
    }

    #[test]
    fn repeat_modes() {
        let mut q = Queue::default();
        q.set(items(2), 0);
        assert_eq!(q.after(1), None);
        q.repeat = Repeat::All;
        assert_eq!(q.after(1), Some(0));
        assert_eq!(q.before(0), Some(1));
        q.repeat = Repeat::One;
        assert_eq!(q.after(1), Some(1));
    }

    #[test]
    fn shuffle_keeps_current_and_restores_order() {
        let mut q = Queue::default();
        q.set(items(20), 5);
        q.set_shuffle(true);
        assert_eq!(q.current_item().map(|i| i.track_id), Some(5));
        assert_eq!(&ids(&q)[..6], &[0, 1, 2, 3, 4, 5]);
        assert_ne!(ids(&q), (0..20).collect::<Vec<_>>());
        q.set_shuffle(false);
        assert_eq!(ids(&q), (0..20).collect::<Vec<_>>());
        assert_eq!(q.current, Some(5));
    }

    #[test]
    fn remove_and_move_track_the_current_item() {
        let mut q = Queue::default();
        q.set(items(5), 2);
        assert!(!q.remove(0));
        assert_eq!(q.current, Some(1));
        q.move_item(1, 3);
        assert_eq!(q.current, Some(3));
        assert_eq!(q.current_item().map(|i| i.track_id), Some(2));
    }
}
