//! Queue-scoped immutable Metal outputs, bounded by live texture bytes.
use anyhow::{Result, bail, ensure};
use gpui::{DevicePixels, Size};
use gpui_apple::custom_draw::{MetalCustomContext, MetalCustomOutput};
use metal::{CommandBuffer, CommandQueue, MTLCommandBufferStatus, Texture};
use std::{collections::HashMap, sync::Mutex};

const OUTPUT_BUDGET: u64 = 64 * 1024 * 1024;
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct Key {
    queue: u64,
    width: i32,
    height: i32,
}
struct Entry {
    texture: Texture,
    command: CommandBuffer,
    // Prevent queue identity from being recycled while this entry exists.
    _queue: CommandQueue,
    used: u64,
    bytes: u64,
}
#[derive(Default)]
struct State {
    entries: HashMap<Key, Entry>,
    bytes: u64,
    clock: u64,
}
pub(crate) struct OutputCache {
    state: Mutex<State>,
    budget: u64,
}
impl Default for OutputCache {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            budget: OUTPUT_BUDGET,
        }
    }
}
impl OutputCache {
    /// Serialize lookup/encoding so concurrent preparations cannot duplicate a miss.
    /// Eviction only drops cache ownership; outstanding outputs/commands retain resources.
    pub(crate) fn prepare(
        &self,
        context: &MetalCustomContext,
        extent: Size<DevicePixels>,
        generate: impl FnOnce() -> Result<(Texture, CommandBuffer)>,
    ) -> Result<(MetalCustomOutput, bool)> {
        ensure!(
            extent.width.0 > 0
                && extent.height.0 > 0
                && extent.width.0 <= 16384
                && extent.height.0 <= 16384,
            "invalid Metal paint extent"
        );
        let bytes = extent.width.0 as u64 * extent.height.0 as u64 * 4;
        ensure!(bytes <= OUTPUT_BUDGET, "Metal paint output exceeds 64 MiB");
        let key = Key {
            queue: context.cache_key(),
            width: extent.width.0,
            height: extent.height.0,
        };
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Metal output cache poisoned"))?;
        state.clock += 1;
        let used = state.clock;
        if let Some(entry) = state.entries.get_mut(&key) {
            let commands = match entry.command.status() {
                MTLCommandBufferStatus::NotEnqueued => vec![entry.command.clone()],
                MTLCommandBufferStatus::Error => {
                    bail!("cached Metal paint preparation failed on the GPU")
                }
                MTLCommandBufferStatus::Enqueued => {
                    bail!("Metal paint command enqueued outside host")
                }
                _ => Vec::new(),
            };
            entry.used = used;
            return Ok((
                MetalCustomOutput {
                    texture: entry.texture.clone(),
                    commands,
                },
                false,
            ));
        }
        let (texture, command) = generate()?;
        // A noncacheable output must not flush useful entries (also permits small test budgets).
        if bytes <= self.budget {
            while state.bytes + bytes > self.budget {
                let oldest = *state
                    .entries
                    .iter()
                    .min_by_key(|(_, e)| e.used)
                    .expect("cache byte accounting")
                    .0;
                let removed = state.entries.remove(&oldest).unwrap();
                state.bytes -= removed.bytes;
            }
            state.entries.insert(
                key,
                Entry {
                    texture: texture.clone(),
                    command: command.clone(),
                    _queue: context.queue.clone(),
                    used,
                    bytes,
                },
            );
            state.bytes += bytes;
        }
        Ok((
            MetalCustomOutput {
                texture,
                commands: vec![command],
            },
            true,
        ))
    }
    pub(crate) fn texture_info(&self) -> Option<(String, u64)> {
        let state = self.state.lock().ok()?;
        let first = state.entries.values().next()?.texture.device();
        let name = if state
            .entries
            .values()
            .all(|e| e.texture.device().registry_id() == first.registry_id())
        {
            first.name().to_owned()
        } else {
            "Multiple Metal devices".to_owned()
        };
        Some((name, state.bytes))
    }
}

#[cfg(test)]
mod tests;
