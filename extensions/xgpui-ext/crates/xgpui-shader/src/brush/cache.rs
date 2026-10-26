//! Bounded compilation caches for point-specialized brush programs and masks.
use super::*;
use std::collections::VecDeque;

pub(super) struct ProgramCache<K, const LIMIT: usize = 128> {
    entries: VecDeque<(K, ShaderProgram<BrushInstance>)>,
}
impl<K, const LIMIT: usize> Default for ProgramCache<K, LIMIT> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }
}
impl<K: PartialEq, const LIMIT: usize> ProgramCache<K, LIMIT> {
    pub fn get(&mut self, key: &K) -> Option<ShaderProgram<BrushInstance>> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        let entry = self.entries.remove(index).unwrap();
        let program = entry.1.clone();
        self.entries.push_back(entry);
        Some(program)
    }
    pub fn insert(&mut self, key: K, program: ShaderProgram<BrushInstance>) {
        if let Some(index) = self.entries.iter().position(|(k, _)| k == &key) {
            self.entries.remove(index);
        }
        self.entries.push_back((key, program));
        while self.entries.len() > LIMIT {
            self.entries.pop_front();
        }
    }
}
