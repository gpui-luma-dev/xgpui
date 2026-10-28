//! Bounded hinting LRU: hash lookup and links between stable slots.
use super::{HINT_LIMIT, HintKey, HintingInstance, Result};
use std::collections::HashMap;

struct Entry {
    key: HintKey,
    hint: HintingInstance,
    previous: Option<usize>,
    next: Option<usize>,
}

pub(super) struct HintCache {
    lookup: HashMap<HintKey, usize>,
    entries: Vec<Entry>,
    oldest: Option<usize>,
    newest: Option<usize>,
}

impl Default for HintCache {
    fn default() -> Self {
        Self {
            lookup: HashMap::with_capacity(HINT_LIMIT),
            entries: Vec::with_capacity(HINT_LIMIT),
            oldest: None,
            newest: None,
        }
    }
}

impl HintCache {
    pub(super) fn get_or_insert(
        &mut self,
        key: HintKey,
        build: impl FnOnce() -> Result<HintingInstance>,
    ) -> Result<&HintingInstance> {
        let slot = if let Some(&slot) = self.lookup.get(&key) {
            self.unlink(slot);
            slot
        } else {
            // A failed build must leave the existing LRU intact.
            let hint = build()?;
            let entry = Entry {
                key: key.clone(),
                hint,
                previous: None,
                next: None,
            };
            let slot = if self.entries.len() == HINT_LIMIT {
                let slot = self.oldest.unwrap();
                self.unlink(slot);
                self.lookup.remove(&self.entries[slot].key);
                self.entries[slot] = entry;
                slot
            } else {
                let slot = self.entries.len();
                self.entries.push(entry);
                slot
            };
            self.lookup.insert(key, slot);
            slot
        };
        self.entries[slot].previous = self.newest;
        self.entries[slot].next = None;
        if let Some(newest) = self.newest {
            self.entries[newest].next = Some(slot);
        } else {
            self.oldest = Some(slot);
        }
        self.newest = Some(slot);
        Ok(&self.entries[slot].hint)
    }

    fn unlink(&mut self, slot: usize) {
        let previous = self.entries[slot].previous;
        let next = self.entries[slot].next;
        if let Some(previous) = previous {
            self.entries[previous].next = next;
        } else {
            self.oldest = next;
        }
        if let Some(next) = next {
            self.entries[next].previous = previous;
        } else {
            self.newest = previous;
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
