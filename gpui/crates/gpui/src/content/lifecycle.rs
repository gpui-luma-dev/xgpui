//! Deterministic accounting shared by future executors; no GPU calls or waits.
use super::*;
use std::{collections::HashSet, sync::Arc};

type Result<T> = std::result::Result<T, ContentError>;
fn transition(message: &str) -> ContentError {
    ContentError::new(ContentErrorKind::InvalidTransition, message)
}

/// Admitted resource policy, independent of a particular driver's queue limit.
#[derive(Clone, Copy, Debug)]
pub struct ExecutionLimits {
    /// Live allocation bytes, including intermediates and retained graph roots.
    pub max_live_bytes: u64,
    /// Concurrent unsubmitted operation recordings. Submission frees this allowance.
    pub max_recording_nodes: usize,
}

/// A dependency-first plan with a feasible serial minimum working set.
///
/// Executors supply actual allocation charges including padding. Shared device
/// caches, staging, and other frames must additionally reserve against a common
/// device budget; this graph-local plan does not claim to account for them.
#[derive(Debug)]
pub struct ExecutionPlan {
    graph: Arc<ContentGraph>,
    allocation_bytes: Vec<u64>,
    consumers: Vec<Vec<usize>>,
    roots: HashSet<usize>,
    peak_bytes: u64,
    limits: ExecutionLimits,
}
impl ExecutionPlan {
    /// Reject impossible working sets before any native allocation or recording.
    /// Every operation is materialized in this initial reference planner.
    pub fn new(
        graph: Arc<ContentGraph>,
        allocation_bytes: Vec<u64>,
        limits: ExecutionLimits,
    ) -> Result<Self> {
        if allocation_bytes.len() != graph.nodes().len() || limits.max_recording_nodes == 0 {
            return Err(ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                "allocation charges must match nodes and recording allowance must be positive",
            ));
        }
        let mut consumers = vec![Vec::new(); graph.nodes().len()];
        for (id, node) in graph.nodes().iter().enumerate() {
            if allocation_bytes[id] < node.descriptor().byte_len() {
                return Err(ContentError::new(
                    ContentErrorKind::InvalidDescriptor,
                    "native allocation charge undercounts logical storage",
                ));
            }
            for dependency in node.dependencies() {
                consumers[*dependency].push(id);
            }
        }
        let roots: HashSet<_> = graph.roots().iter().copied().collect();
        let mut remaining: Vec<_> = consumers.iter().map(Vec::len).collect();
        let mut live = 0u64;
        let mut peak_bytes = 0;
        for (id, node) in graph.nodes().iter().enumerate() {
            live = live.checked_add(allocation_bytes[id]).ok_or_else(|| {
                ContentError::new(ContentErrorKind::ResourceLimit, "working set overflow")
            })?;
            peak_bytes = peak_bytes.max(live);
            for dependency in node.dependencies() {
                remaining[*dependency] -= 1;
                if remaining[*dependency] == 0 && !roots.contains(dependency) {
                    live -= allocation_bytes[*dependency];
                }
            }
        }
        if peak_bytes > limits.max_live_bytes {
            return Err(ContentError::new(
                ContentErrorKind::ResourceLimit,
                format!(
                    "serial working set needs {peak_bytes} bytes; budget is {}",
                    limits.max_live_bytes
                ),
            ));
        }
        Ok(Self {
            graph,
            allocation_bytes,
            consumers,
            roots,
            peak_bytes,
            limits,
        })
    }
    /// Serial peak including all retained root images; not an optimal reorder bound.
    pub fn peak_bytes(&self) -> u64 {
        self.peak_bytes
    }
    /// Validated logical graph to lower.
    pub fn graph(&self) -> &ContentGraph {
        &self.graph
    }
    /// Begin accounting with no allocations reserved.
    pub fn start(self) -> ExecutionTracker {
        self.start_with_owner()
    }
    /// Choose an executor-specific owner type, including thread-affine `Rc` owners.
    pub fn start_with_owner<Owner: Clone>(self) -> ExecutionTracker<Owner> {
        let nodes = (0..self.graph.nodes().len())
            .map(|_| NodeExecution {
                state: ExecutionState::Planned,
                invalid: false,
                reserved: false,
                owners: Vec::new(),
            })
            .collect();
        ExecutionTracker {
            plan: self,
            nodes,
            live_bytes: 0,
            recording_nodes: 0,
            retain_roots: true,
        }
    }
}

/// Native work state, distinct from whether its result has been invalidated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionState {
    /// No resource admission yet.
    Planned,
    /// Resources reserved, recording not yet submitted/discarded.
    Encoding,
    /// Work submitted; owners must survive even if its result is invalid.
    Submitted,
    /// Completion observed successfully.
    Completed,
    /// Completion observed with an invalid result.
    Failed,
    /// No submission will access the resources.
    Abandoned,
}
impl ExecutionState {
    fn retired(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Abandoned)
    }
}
/// Why admission must resume later, without waiting or partial reservation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionPressure {
    /// Dependencies are not yet submitted or completed.
    Dependencies,
    /// An earlier planned operation must be admitted first to preserve the
    /// working-set proof. Ready independent roots cannot consume its allowance.
    ExecutionOrder,
    /// Unsubmitted recordings must be flushed/discarded first.
    RecordingAllowance,
    /// Completion or consumer retirement must release live storage.
    LiveBytes,
}
/// Immediate, nonblocking admission outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    /// Entire output working allocation reserved.
    Admitted,
    /// No state or reservation changed.
    Deferred(AdmissionPressure),
}
struct NodeExecution<Owner> {
    state: ExecutionState,
    invalid: bool,
    reserved: bool,
    owners: Vec<Owner>,
}

/// Retained output/input owners for one native submission. Move this value into
/// the native completion callback before committing work; release it only once
/// GPU use ends. It survives dropping the renderer or its execution tracker.
#[must_use = "retain the submission lease in the GPU completion callback until work ends"]
pub struct SubmissionLease<Owner = Arc<dyn Send + Sync>> {
    owners: Vec<Owner>,
}
impl<Owner> SubmissionLease<Owner> {
    /// Number of retained owner references, including resource inputs.
    pub fn owner_count(&self) -> usize {
        self.owners.len()
    }
}

/// Serial-owner lifecycle accounting with explicit completion/discard acknowledgments.
///
/// This is not a background scheduler. Executors drive it from actual submission
/// and completion events. Submission leases must be installed in native completion callbacks before
/// actual submission. Accounting services can retain this tracker to process
/// completion across teardown; leases protect owners even if the tracker is dropped.
pub struct ExecutionTracker<Owner = Arc<dyn Send + Sync>> {
    plan: ExecutionPlan,
    nodes: Vec<NodeExecution<Owner>>,
    live_bytes: u64,
    recording_nodes: usize,
    retain_roots: bool,
}
impl<Owner: Clone> ExecutionTracker<Owner> {
    fn node(&self, id: usize) -> Result<&NodeExecution<Owner>> {
        self.nodes
            .get(id)
            .ok_or_else(|| transition("invalid execution node"))
    }
    /// Current native-work state.
    pub fn state(&self, id: usize) -> Result<ExecutionState> {
        Ok(self.node(id)?.state)
    }
    /// Whether a successful realization can still be published/reused.
    pub fn is_valid(&self, id: usize) -> Result<bool> {
        let node = self.node(id)?;
        Ok(node.reserved
            && !node.invalid
            && matches!(
                node.state,
                ExecutionState::Submitted | ExecutionState::Completed
            ))
    }
    /// Currently reserved live bytes.
    pub fn live_bytes(&self) -> u64 {
        self.live_bytes
    }
    /// Current unsubmitted operation recordings.
    pub fn recording_nodes(&self) -> usize {
        self.recording_nodes
    }
    /// Reserve an entire output or defer without changing any accounting.
    pub fn try_admit(&mut self, id: usize) -> Result<Admission> {
        let node = self.node(id)?;
        if node.state != ExecutionState::Planned || node.invalid {
            return Err(transition("only valid planned work can be admitted"));
        }
        for dependency in self.plan.graph.nodes()[id].dependencies() {
            let input = &self.nodes[*dependency];
            if input.invalid {
                return Err(transition("cannot admit a consumer of invalid content"));
            }
            if !matches!(
                input.state,
                ExecutionState::Submitted | ExecutionState::Completed
            ) {
                return Ok(Admission::Deferred(AdmissionPressure::Dependencies));
            }
        }
        if self.nodes[..id]
            .iter()
            .any(|node| node.state == ExecutionState::Planned)
        {
            return Ok(Admission::Deferred(AdmissionPressure::ExecutionOrder));
        }
        if self.recording_nodes >= self.plan.limits.max_recording_nodes {
            return Ok(Admission::Deferred(AdmissionPressure::RecordingAllowance));
        }
        let charge = self.plan.allocation_bytes[id];
        if charge
            > self
                .plan
                .limits
                .max_live_bytes
                .saturating_sub(self.live_bytes)
        {
            return Ok(Admission::Deferred(AdmissionPressure::LiveBytes));
        }
        self.live_bytes += charge;
        self.recording_nodes += 1;
        self.nodes[id].reserved = true;
        self.nodes[id].state = ExecutionState::Encoding;
        Ok(Admission::Admitted)
    }
    /// Transfer a native backing or external pool lease into admitted ownership.
    pub fn retain_owner(&mut self, id: usize, owner: Owner) -> Result<()> {
        if self.node(id)?.state != ExecutionState::Encoding || self.nodes[id].invalid {
            return Err(transition(
                "owners must be attached during admitted recording",
            ));
        }
        self.nodes[id].owners.push(owner);
        Ok(())
    }
    /// Prepare submission accounting and its completion lease. Install the lease
    /// in the native callback, then submit in the same serial-owner scope. If
    /// native submission fails, acknowledge completion with `success = false`.
    pub fn submitted(&mut self, id: usize) -> Result<SubmissionLease<Owner>> {
        if self.node(id)?.state != ExecutionState::Encoding || self.nodes[id].invalid {
            return Err(transition("cannot submit non-recording or invalid work"));
        }
        let mut owners = self.nodes[id].owners.clone();
        for dependency in self.plan.graph.nodes()[id].dependencies() {
            owners.extend(self.nodes[*dependency].owners.iter().cloned());
        }
        self.nodes[id].state = ExecutionState::Submitted;
        self.recording_nodes -= 1;
        Ok(SubmissionLease { owners })
    }
    /// Observe that GPU use has ended. Failure invalidates all transitive consumers.
    pub fn completed(&mut self, id: usize, success: bool) -> Result<()> {
        if self.node(id)?.state != ExecutionState::Submitted {
            return Err(transition("completion requires submitted work"));
        }
        if !success {
            self.invalidate(id);
        }
        self.nodes[id].state = if self.nodes[id].invalid {
            ExecutionState::Failed
        } else {
            ExecutionState::Completed
        };
        self.release_retired();
        Ok(())
    }
    /// Acknowledge that an unsubmitted native recorder has already been discarded.
    /// Submitted work cannot be abandoned; it requires completion acknowledgment.
    pub fn discarded(&mut self, id: usize) -> Result<()> {
        let state = self.node(id)?.state;
        if !matches!(state, ExecutionState::Encoding | ExecutionState::Planned) {
            return Err(transition("submitted or terminal work cannot be discarded"));
        }
        if state == ExecutionState::Encoding {
            self.recording_nodes -= 1;
        }
        self.invalidate(id);
        self.nodes[id].state = ExecutionState::Abandoned;
        self.release_retired();
        Ok(())
    }
    fn invalidate(&mut self, id: usize) {
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            if self.nodes[id].invalid {
                continue;
            }
            let node = &mut self.nodes[id];
            node.invalid = true;
            match node.state {
                ExecutionState::Planned => node.state = ExecutionState::Abandoned,
                ExecutionState::Completed => node.state = ExecutionState::Failed,
                _ => {} // Encoding needs discard; Submitted needs completion.
            }
            stack.extend(self.plan.consumers[id].iter().copied());
        }
    }
    /// Stop retaining graph roots for presentation/cache. Does not cancel GPU work.
    pub fn release_roots(&mut self) {
        self.retain_roots = false;
        self.release_retired();
    }
    /// Cancel only work not yet admitted. Active encoders still require discard.
    pub fn cancel_unstarted(&mut self) {
        for id in 0..self.nodes.len() {
            if self.nodes[id].state == ExecutionState::Planned {
                self.invalidate(id);
            }
        }
        self.release_roots();
    }
    fn release_retired(&mut self) {
        for id in 0..self.nodes.len() {
            let node = &self.nodes[id];
            let keep_root = self.retain_roots && self.plan.roots.contains(&id) && !node.invalid;
            if node.reserved
                && node.state.retired()
                && !keep_root
                && self.plan.consumers[id]
                    .iter()
                    .all(|consumer| self.nodes[*consumer].state.retired())
            {
                self.nodes[id].reserved = false;
                self.live_bytes -= self.plan.allocation_bytes[id];
                self.nodes[id].owners.clear();
            }
        }
    }
    /// All submitted/recorded work has ended; a completion owner can now retire.
    pub fn is_quiescent(&self) -> bool {
        self.nodes.iter().all(|node| node.state.retired())
    }
}

/// Window-owned generation check, independent of content or native resource identity.
#[derive(Debug)]
pub struct PresentationEpoch {
    identity: u64,
    current: u64,
}
/// A requested presentation generation, scoped to its owning epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameGeneration {
    epoch: u64,
    revision: u64,
}
impl Default for PresentationEpoch {
    fn default() -> Self {
        Self {
            identity: next_content_identity(),
            current: 0,
        }
    }
}
impl PresentationEpoch {
    /// Supersede older requests. Generations from another epoch cannot publish.
    pub fn request(&mut self) -> FrameGeneration {
        self.current = self
            .current
            .checked_add(1)
            .expect("presentation generation exhausted");
        FrameGeneration {
            epoch: self.identity,
            revision: self.current,
        }
    }
    /// Whether this request is still eligible for publication.
    pub fn is_current(&self, generation: FrameGeneration) -> bool {
        generation.epoch == self.identity && generation.revision == self.current
    }
}
