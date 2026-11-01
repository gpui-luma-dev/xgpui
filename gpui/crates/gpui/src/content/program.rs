//! Validated WGSL artifacts. Nothing in this module creates native GPU objects.
use super::*;
use naga::{AddressSpace, ImageClass, ScalarKind, ShaderStage, TypeInner};
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

type Result<T> = std::result::Result<T, ContentError>;
fn shader_error(message: impl Into<String>) -> ContentError {
    ContentError::new(ContentErrorKind::ShaderValidation, message)
}
fn unsupported(message: impl Into<String>) -> ContentError {
    ContentError::new(ContentErrorKind::UnsupportedCapability, message)
}

/// Declared image interpretation by WGSL resource name. Reflection checks the
/// resource shape; shader authors remain responsible for actual pixel meaning.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ImageContracts(BTreeMap<String, ImageInterpretation>);
impl ImageContracts {
    /// No image contracts yet; valid for programs without image resources.
    pub fn new() -> Self {
        Self::default()
    }
    /// Set an image's declared color/alpha domain; replaces a previous value.
    pub fn image(mut self, name: impl Into<String>, interpretation: ImageInterpretation) -> Self {
        self.0.insert(name.into(), interpretation);
        self
    }
}
/// Portable resource binding address, reflected from WGSL annotations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingAddress {
    /// Bind group index.
    pub group: u32,
    /// Binding index within the group.
    pub binding: u32,
}
bitflags::bitflags! {
    /// Selected stages which actually reference a reflected binding.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ProgramStages: u8 {
        /// Vertex use.
        const VERTEX = 1;
        /// Fragment use.
        const FRAGMENT = 2;
        /// Compute use.
        const COMPUTE = 4;
    }
}
/// Declared storage read/write access, independent of any native API flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StorageAccess {
    /// Immutable resource input.
    ReadOnly,
    /// Output accessible only for writes.
    WriteOnly,
    /// Fresh output that can also be read during execution.
    ReadWrite,
}
/// Reflected dynamically sized tail, including struct-prefix bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuntimeArrayLayout {
    /// Byte offset of the first element.
    pub offset: u32,
    /// Byte distance between elements.
    pub stride: u32,
}
/// Reflected storage allocation requirements, including runtime-array layout.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StorageBufferLayout {
    minimum_bytes: u32,
    alignment: u32,
    runtime_array: Option<RuntimeArrayLayout>,
}
impl StorageBufferLayout {
    /// Minimum shader-accessible binding length, including at least one runtime element.
    pub fn minimum_bytes(&self) -> u32 {
        self.minimum_bytes
    }
    /// Natural WGSL byte alignment (native binding-offset alignment is separate).
    pub fn alignment(&self) -> u32 {
        self.alignment
    }
    /// Optional unsized array tail.
    pub fn runtime_array(&self) -> Option<RuntimeArrayLayout> {
        self.runtime_array
    }
    /// Check binding length against the reflected struct and complete tail elements.
    pub fn validate_byte_len(&self, byte_len: u64) -> Result<()> {
        let valid = byte_len >= u64::from(self.minimum_bytes)
            && self.runtime_array.is_none_or(|tail| {
                byte_len
                    .checked_sub(u64::from(tail.offset))
                    .is_some_and(|len| len.is_multiple_of(u64::from(tail.stride)))
            });
        if valid {
            Ok(())
        } else {
            Err(ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                "storage buffer binding is too small or has an incomplete runtime element",
            ))
        }
    }
}
/// Type and semantic contract of a used global resource.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ProgramBindingKind {
    /// A reflected typed uniform struct.
    Uniform(UniformLayout),
    /// Non-multisampled two-dimensional floating-point sampled texture.
    SampledImage(ImageInterpretation),
    /// Ordinary (non-comparison) sampler; filtering is chosen when binding it.
    Sampler,
    /// Storage buffer shape and access.
    StorageBuffer {
        /// Shader allocation/layout requirements.
        layout: StorageBufferLayout,
        /// Declared access.
        access: StorageAccess,
    },
    /// Two-dimensional storage texture with explicit interpretation.
    StorageImage {
        /// Exact shader-declared storage format.
        format: PixelFormat,
        /// Declared access.
        access: StorageAccess,
        /// Pixel meaning supplied by the program author.
        interpretation: ImageInterpretation,
    },
}
/// Checked resource declaration used by one or more selected entry points.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProgramBinding {
    name: Arc<str>,
    address: BindingAddress,
    stages: ProgramStages,
    kind: ProgramBindingKind,
}
impl ProgramBinding {
    /// WGSL global resource name used by portable pass bindings.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Reflected bind group and binding index.
    pub fn address(&self) -> BindingAddress {
        self.address
    }
    /// Selected stages that reference the resource, including through helpers.
    pub fn stages(&self) -> ProgramStages {
        self.stages
    }
    /// Exact resource kind and declared interpretation.
    pub fn kind(&self) -> &ProgramBindingKind {
        &self.kind
    }
}
/// One vertex attribute required by a selected mesh vertex entry point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VertexInput {
    /// Shader location.
    pub location: u32,
    /// Required scalar type and lane count.
    pub ty: ShaderValueType,
}
/// An image/sampler pair actually used by selected stages, including helper calls.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SamplingPair {
    /// Image resource name.
    pub image: Arc<str>,
    /// Sampler resource name.
    pub sampler: Arc<str>,
}
/// Executor-reported compute limits. Construct from capabilities, not backend names.
#[derive(Clone, Copy, Debug)]
pub struct ComputeLimits {
    /// Maximum workgroup dimensions per axis.
    pub max_workgroup_size: [u32; 3],
    /// Maximum invocations in one workgroup.
    pub max_workgroup_invocations: u32,
    /// Maximum workgroups dispatched per axis.
    pub max_dispatch: [u32; 3],
    /// Maximum used workgroup-shared allocation bytes.
    pub max_workgroup_memory_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ProgramKind {
    Render {
        vertex: Arc<str>,
        fragment: Arc<str>,
        output: ImageInterpretation,
        fullscreen: bool,
    },
    Compute {
        entry: Arc<str>,
    },
}
#[derive(Debug, PartialEq, Eq, Hash)]
struct ProgramKey {
    wgsl: Arc<str>,
    kind: ProgramKind,
    images: ImageContracts,
}
#[derive(Debug)]
struct ProgramData {
    key: ProgramKey,
    bindings: Vec<ProgramBinding>,
    uniform_layouts: BTreeMap<Arc<str>, Result<UniformLayout>>,
    vertex_inputs: Vec<VertexInput>,
    workgroup_size: Option<[u32; 3]>,
    workgroup_bytes: u64,
    sampling_pairs: Vec<SamplingPair>,
}
/// Immutable validated WGSL and reflected interface. Equal reconstructed programs
/// compare structurally; there is no caller-controlled numeric program identity.
#[derive(Clone, Debug)]
pub struct PortableProgram(Arc<ProgramData>);
impl PartialEq for PortableProgram {
    fn eq(&self, other: &Self) -> bool {
        self.0.key == other.0.key
    }
}
impl Eq for PortableProgram {}
impl Hash for PortableProgram {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.key.hash(state);
    }
}
impl PortableProgram {
    /// Source-size policy for CPU validation; not a backend shader limit.
    pub const MAX_SOURCE_BYTES: usize = 256 * 1024;
    /// Maximum reflected bytes per typed uniform block.
    pub const MAX_UNIFORM_BYTES: u32 = 64 * 1024;

    /// Validate custom vertex and fragment entry points and their linked interface.
    /// Initial render programs have one vec4<f32> color output at location zero,
    /// no depth output, and read-only storage resources.
    pub fn render(
        source: impl Into<Arc<str>>,
        vertex: impl Into<Arc<str>>,
        fragment: impl Into<Arc<str>>,
        output: ImageInterpretation,
        images: ImageContracts,
    ) -> Result<Self> {
        Self::validate(ProgramKey {
            wgsl: source.into(),
            kind: ProgramKind::Render {
                vertex: vertex.into(),
                fragment: fragment.into(),
                output,
                fullscreen: false,
            },
            images,
        })
    }
    /// Supply the portable fullscreen triangle vertex entry point. The fragment
    /// may read location-zero vec2<f32> UV (top-left origin) and builtin position.
    /// The caller supplies a complete WGSL fragment module, not a native shader.
    pub fn fullscreen(
        source: impl Into<Arc<str>>,
        fragment: impl Into<Arc<str>>,
        output: ImageInterpretation,
        images: ImageContracts,
    ) -> Result<Self> {
        let source = source.into();
        if source.len() > Self::MAX_SOURCE_BYTES {
            return Err(ContentError::new(
                ContentErrorKind::ResourceLimit,
                "shader source exceeds validation budget",
            ));
        }
        let wgsl = format!("{FULLSCREEN_VERTEX}\n{source}").into();
        Self::validate(ProgramKey {
            wgsl,
            kind: ProgramKind::Render {
                vertex: Arc::from("_xgpui_content_vertex"),
                fragment: fragment.into(),
                output,
                fullscreen: true,
            },
            images,
        })
    }
    /// Validate a selected compute entry point with fixed workgroup dimensions.
    /// Device compute support and dispatch limits are checked separately.
    pub fn compute(
        source: impl Into<Arc<str>>,
        entry: impl Into<Arc<str>>,
        images: ImageContracts,
    ) -> Result<Self> {
        Self::validate(ProgramKey {
            wgsl: source.into(),
            kind: ProgramKind::Compute {
                entry: entry.into(),
            },
            images,
        })
    }
    /// Validated portable source (including generated fullscreen vertex code).
    pub fn wgsl(&self) -> &str {
        &self.0.key.wgsl
    }
    /// Resources used by selected stages, sorted by binding address.
    pub fn bindings(&self) -> &[ProgramBinding] {
        &self.0.bindings
    }
    /// Reflected pairings used for filtering/mip compatibility validation.
    pub fn sampling_pairs(&self) -> &[SamplingPair] {
        &self.0.sampling_pairs
    }
    /// Named used resource; unused globals are deliberately not required bindings.
    pub fn binding(&self, name: &str) -> Option<&ProgramBinding> {
        self.bindings().iter().find(|b| b.name() == name)
    }
    /// Layout of a declared uniform, including declarations unused by the selected
    /// entry points. Unused declarations never become required execution bindings.
    /// This validates parameter snapshots even when a compiler removes their use.
    pub fn uniform_layout(&self, name: &str) -> Result<&UniformLayout> {
        self.0
            .uniform_layouts
            .get(name)
            .ok_or_else(|| {
                ContentError::new(
                    ContentErrorKind::InvalidDescriptor,
                    "unknown uniform declaration",
                )
            })?
            .as_ref()
            .map_err(Clone::clone)
    }
    /// Reflected mesh vertex inputs, excluding builtin vertex/instance indices.
    pub fn vertex_inputs(&self) -> &[VertexInput] {
        &self.0.vertex_inputs
    }
    /// Selected render entry points, or none for compute programs.
    pub fn render_entries(&self) -> Option<(&str, &str)> {
        match &self.0.key.kind {
            ProgramKind::Render {
                vertex, fragment, ..
            } => Some((vertex, fragment)),
            _ => None,
        }
    }
    /// Selected compute entry point, or none for render programs.
    pub fn compute_entry(&self) -> Option<&str> {
        match &self.0.key.kind {
            ProgramKind::Compute { entry } => Some(entry),
            _ => None,
        }
    }
    /// Whether the framework supplied the fullscreen vertex interface.
    pub fn is_fullscreen(&self) -> bool {
        matches!(
            self.0.key.kind,
            ProgramKind::Render {
                fullscreen: true,
                ..
            }
        )
    }
    /// Declared fragment output interpretation, separate from storage format.
    pub fn fragment_output(&self) -> Option<ImageInterpretation> {
        match self.0.key.kind {
            ProgramKind::Render { output, .. } => Some(output),
            _ => None,
        }
    }
    /// Fixed reflected compute workgroup size.
    pub fn workgroup_size(&self) -> Option<[u32; 3]> {
        self.0.workgroup_size
    }
    /// Portable shared-memory charge for selected-stage variables, including
    /// helpers, with each variable rounded up to at least 16-byte granularity.
    pub fn workgroup_memory_bytes(&self) -> u64 {
        self.0.workgroup_bytes
    }
    /// Check compute availability and dimensions before resource admission.
    pub fn validate_dispatch(
        &self,
        dispatch: [u32; 3],
        limits: Option<&ComputeLimits>,
    ) -> Result<()> {
        let group = self.workgroup_size().ok_or_else(|| {
            ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                "render programs cannot be dispatched",
            )
        })?;
        let limits = limits.ok_or_else(|| unsupported("compute execution is unavailable"))?;
        if dispatch.contains(&0) {
            return Err(ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                "dispatch dimensions must be positive",
            ));
        }
        let invocations = group
            .iter()
            .try_fold(1u64, |total, v| total.checked_mul(u64::from(*v)));
        if group
            .iter()
            .zip(limits.max_workgroup_size)
            .any(|(v, max)| *v == 0 || *v > max)
            || invocations.is_none_or(|n| n > u64::from(limits.max_workgroup_invocations))
            || dispatch
                .iter()
                .zip(limits.max_dispatch)
                .any(|(v, max)| *v > max)
            || self.workgroup_memory_bytes() > limits.max_workgroup_memory_bytes
        {
            return Err(unsupported(
                "workgroup, shared memory, or dispatch exceeds compute limits",
            ));
        }
        Ok(())
    }

    fn validate(key: ProgramKey) -> Result<Self> {
        if key.wgsl.len() > Self::MAX_SOURCE_BYTES {
            return Err(ContentError::new(
                ContentErrorKind::ResourceLimit,
                "shader source exceeds validation budget",
            ));
        }
        let module = naga::front::wgsl::parse_str(&key.wgsl)
            .map_err(|e| shader_error(e.emit_to_string(&key.wgsl)))?;
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .map_err(|e| shader_error(e.emit_to_string(&key.wgsl)))?;
        if !module.overrides.is_empty() {
            return Err(unsupported(
                "specialization overrides require a future explicit program snapshot contract",
            ));
        }
        let mut layouter = naga::proc::Layouter::default();
        layouter
            .update(module.to_ctx())
            .map_err(|e| shader_error(e.to_string()))?;
        let (entries, vertex_inputs, workgroup_size) = match &key.kind {
            ProgramKind::Render {
                vertex,
                fragment,
                output,
                ..
            } => {
                if output.color == ColorEncoding::Coverage && output.alpha != AlphaMode::Opaque {
                    return Err(ContentError::new(
                        ContentErrorKind::InvalidDescriptor,
                        "coverage output requires opaque interpretation",
                    ));
                }
                let vertex = entry_point(&module, vertex, ShaderStage::Vertex)?;
                let fragment = entry_point(&module, fragment, ShaderStage::Fragment)?;
                let inputs = link_render(&module, vertex, fragment)?;
                (
                    vec![
                        (vertex, ProgramStages::VERTEX),
                        (fragment, ProgramStages::FRAGMENT),
                    ],
                    inputs,
                    None,
                )
            }
            ProgramKind::Compute { entry } => {
                let index = entry_point(&module, entry, ShaderStage::Compute)?;
                let entry = &module.entry_points[index];
                if entry.workgroup_size_overrides.is_some() {
                    return Err(unsupported("compute workgroup overrides are not supported"));
                }
                (
                    vec![(index, ProgramStages::COMPUTE)],
                    Vec::new(),
                    Some(entry.workgroup_size),
                )
            }
        };
        let uniform_layouts = module
            .global_variables
            .iter()
            .filter(|(_, global)| global.space == AddressSpace::Uniform)
            .filter_map(|(_, global)| {
                global.name.as_deref().map(|name| {
                    (
                        Arc::from(name),
                        reflect_uniform(&module, &layouter, global.ty),
                    )
                })
            })
            .collect();
        let mut bindings = Vec::new();
        let mut workgroup_bytes = 0u64;
        let mut image_names = Vec::new();
        for (handle, global) in module.global_variables.iter() {
            let mut stages = ProgramStages::empty();
            for (index, stage) in &entries {
                if !info.get_entry_point(*index)[handle].is_empty() {
                    stages |= *stage;
                }
            }
            if stages.is_empty() {
                continue;
            }
            if global.space == AddressSpace::WorkGroup {
                let charge = u64::from(layouter[global.ty].size).div_ceil(16) * 16;
                workgroup_bytes = workgroup_bytes
                    .checked_add(charge)
                    .ok_or_else(|| shader_error("workgroup memory size overflow"))?;
                continue;
            }
            if matches!(global.space, AddressSpace::Private) {
                continue;
            }
            let address = global
                .binding
                .as_ref()
                .ok_or_else(|| unsupported("used resource has no portable binding address"))?;
            let name: Arc<str> = global
                .name
                .as_deref()
                .ok_or_else(|| shader_error("used resource requires a name"))?
                .into();
            let kind = reflect_binding(
                &module,
                &layouter,
                global,
                &key.images,
                workgroup_size.is_some(),
            )?;
            if matches!(
                kind,
                ProgramBindingKind::SampledImage(_) | ProgramBindingKind::StorageImage { .. }
            ) {
                image_names.push(name.to_string());
            }
            bindings.push(ProgramBinding {
                name,
                address: BindingAddress {
                    group: address.group,
                    binding: address.binding,
                },
                stages,
                kind,
            });
        }
        if key.images.0.len() != image_names.len()
            || key.images.0.keys().any(|name| !image_names.contains(name))
        {
            return Err(ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                "image contracts must match exactly the selected stages' used image resources",
            ));
        }
        bindings.sort_by_key(ProgramBinding::address);
        if bindings
            .windows(2)
            .any(|pair| pair[0].address == pair[1].address)
        {
            return Err(shader_error(
                "selected stages use distinct globals at the same binding address",
            ));
        }
        let mut sampling_pairs = Vec::new();
        for (entry, _) in &entries {
            for pair in &info.get_entry_point(*entry).sampling_set {
                sampling_pairs.push(SamplingPair {
                    image: module.global_variables[pair.image]
                        .name
                        .as_deref()
                        .unwrap()
                        .into(),
                    sampler: module.global_variables[pair.sampler]
                        .name
                        .as_deref()
                        .unwrap()
                        .into(),
                });
            }
        }
        sampling_pairs.sort();
        sampling_pairs.dedup();
        Ok(Self(Arc::new(ProgramData {
            key,
            bindings,
            uniform_layouts,
            vertex_inputs,
            workgroup_size,
            workgroup_bytes,
            sampling_pairs,
        })))
    }
}

fn entry_point(module: &naga::Module, name: &str, stage: ShaderStage) -> Result<usize> {
    module
        .entry_points
        .iter()
        .position(|e| e.name == name && e.stage == stage)
        .ok_or_else(|| shader_error(format!("missing {stage:?} entry point `{name}`")))
}
fn value_type(module: &naga::Module, ty: naga::Handle<naga::Type>) -> Result<ShaderValueType> {
    let (scalar, lanes) = match module.types[ty].inner {
        TypeInner::Scalar(scalar) => (scalar, 1),
        TypeInner::Vector { scalar, size } => (scalar, size as u8),
        _ => {
            return Err(unsupported(
                "interface values require 32-bit scalar/vector types",
            ));
        }
    };
    if scalar.width != 4 {
        return Err(unsupported("interface scalar width must be 32 bits"));
    }
    match scalar.kind {
        ScalarKind::Float => Ok(ShaderValueType::Float(lanes)),
        ScalarKind::Sint => Ok(ShaderValueType::Sint(lanes)),
        ScalarKind::Uint => Ok(ShaderValueType::Uint(lanes)),
        _ => Err(unsupported("unsupported interface scalar type")),
    }
}
#[derive(PartialEq, Eq)]
struct IoField {
    ty: ShaderValueType,
    binding: naga::Binding,
}
fn collect_io(
    module: &naga::Module,
    ty: naga::Handle<naga::Type>,
    binding: Option<&naga::Binding>,
    fields: &mut BTreeMap<u32, IoField>,
    allow_builtins: bool,
) -> Result<()> {
    match binding {
        Some(naga::Binding::Location {
            location,
            blend_src,
            per_primitive,
            ..
        }) => {
            if blend_src.is_some() || *per_primitive {
                return Err(unsupported(
                    "dual-source/per-primitive stage interfaces are not supported",
                ));
            }
            if fields
                .insert(
                    *location,
                    IoField {
                        ty: value_type(module, ty)?,
                        binding: binding.unwrap().clone(),
                    },
                )
                .is_some()
            {
                return Err(shader_error("duplicate stage interface location"));
            }
        }
        Some(naga::Binding::BuiltIn(_)) if allow_builtins => {}
        Some(naga::Binding::BuiltIn(_)) => {
            return Err(unsupported(
                "fragment depth and other builtin outputs require an explicit future render-state contract",
            ));
        }
        None => {
            if let TypeInner::Struct { members, .. } = &module.types[ty].inner {
                for member in members {
                    collect_io(
                        module,
                        member.ty,
                        member.binding.as_ref(),
                        fields,
                        allow_builtins,
                    )?;
                }
            } else {
                return Err(shader_error("stage value has no location/builtin binding"));
            }
        }
    }
    Ok(())
}
fn link_render(module: &naga::Module, vertex: usize, fragment: usize) -> Result<Vec<VertexInput>> {
    let vertex = &module.entry_points[vertex].function;
    let fragment = &module.entry_points[fragment].function;
    let mut vertex_inputs = BTreeMap::new();
    let mut outputs = BTreeMap::new();
    let mut inputs = BTreeMap::new();
    let mut color = BTreeMap::new();
    for argument in &vertex.arguments {
        collect_io(
            module,
            argument.ty,
            argument.binding.as_ref(),
            &mut vertex_inputs,
            true,
        )?;
    }
    if let Some(result) = &vertex.result {
        collect_io(
            module,
            result.ty,
            result.binding.as_ref(),
            &mut outputs,
            true,
        )?;
    }
    for argument in &fragment.arguments {
        collect_io(
            module,
            argument.ty,
            argument.binding.as_ref(),
            &mut inputs,
            true,
        )?;
    }
    for (location, input) in inputs {
        if outputs.get(&location) != Some(&input) {
            return Err(shader_error(format!(
                "vertex/fragment interface mismatch at location {location}"
            )));
        }
    }
    if let Some(result) = &fragment.result {
        collect_io(
            module,
            result.ty,
            result.binding.as_ref(),
            &mut color,
            false,
        )?;
    }
    if color.len() != 1
        || color
            .get(&0)
            .is_none_or(|field| field.ty != ShaderValueType::Float(4))
    {
        return Err(unsupported(
            "render programs require one location-zero vec4<f32> color output",
        ));
    }
    Ok(vertex_inputs
        .into_iter()
        .map(|(location, field)| VertexInput {
            location,
            ty: field.ty,
        })
        .collect())
}
fn reflect_access(access: naga::StorageAccess) -> StorageAccess {
    match (
        access.contains(naga::StorageAccess::LOAD),
        access.contains(naga::StorageAccess::STORE),
    ) {
        (true, false) => StorageAccess::ReadOnly,
        (false, true) => StorageAccess::WriteOnly,
        _ => StorageAccess::ReadWrite,
    }
}
fn reflect_binding(
    module: &naga::Module,
    layouter: &naga::proc::Layouter,
    global: &naga::GlobalVariable,
    images: &ImageContracts,
    compute: bool,
) -> Result<ProgramBindingKind> {
    let image_interpretation = || {
        let name = global.name.as_deref().unwrap_or("");
        let interpretation = images.0.get(name).copied().ok_or_else(|| {
            ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                format!("missing image contract for `{name}`"),
            )
        })?;
        if interpretation.color == ColorEncoding::Coverage
            && interpretation.alpha != AlphaMode::Opaque
        {
            return Err(ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                "coverage images require opaque interpretation",
            ));
        }
        Ok(interpretation)
    };
    match global.space {
        AddressSpace::Uniform => Ok(ProgramBindingKind::Uniform(reflect_uniform(
            module, layouter, global.ty,
        )?)),
        AddressSpace::Storage { access } => {
            let access = reflect_access(access);
            if !compute && access != StorageAccess::ReadOnly {
                return Err(unsupported("render storage buffers must be read-only"));
            }
            let ty = &module.types[global.ty].inner;
            let runtime_array = match ty {
                TypeInner::Array {
                    size: naga::ArraySize::Dynamic,
                    stride,
                    ..
                } => Some(RuntimeArrayLayout {
                    offset: 0,
                    stride: *stride,
                }),
                TypeInner::Struct { members, .. } => members.last().and_then(|member| match module
                    .types[member.ty]
                    .inner
                {
                    TypeInner::Array {
                        size: naga::ArraySize::Dynamic,
                        stride,
                        ..
                    } => Some(RuntimeArrayLayout {
                        offset: member.offset,
                        stride,
                    }),
                    _ => None,
                }),
                _ => None,
            };
            let layout = layouter[global.ty];
            Ok(ProgramBindingKind::StorageBuffer {
                layout: StorageBufferLayout {
                    minimum_bytes: layout.size,
                    alignment: layout.alignment * 1,
                    runtime_array,
                },
                access,
            })
        }
        AddressSpace::Handle => match module.types[global.ty].inner {
            TypeInner::Sampler { comparison: false } => Ok(ProgramBindingKind::Sampler),
            TypeInner::Image {
                dim: naga::ImageDimension::D2,
                arrayed: false,
                class:
                    ImageClass::Sampled {
                        kind: ScalarKind::Float,
                        multi: false,
                    },
            } => Ok(ProgramBindingKind::SampledImage(image_interpretation()?)),
            TypeInner::Image {
                dim: naga::ImageDimension::D2,
                arrayed: false,
                class: ImageClass::Storage { format, access },
            } => {
                let access = reflect_access(access);
                if !compute && access != StorageAccess::ReadOnly {
                    return Err(unsupported("render storage images must be read-only"));
                }
                let format = match format {
                    naga::StorageFormat::Rgba8Unorm => PixelFormat::Rgba8,
                    naga::StorageFormat::Bgra8Unorm => PixelFormat::Bgra8,
                    naga::StorageFormat::Rgba16Float => PixelFormat::Rgba16Float,
                    naga::StorageFormat::R8Unorm => PixelFormat::R8,
                    _ => {
                        return Err(unsupported(
                            "storage texture format is outside the current portable image formats",
                        ));
                    }
                };
                let interpretation = image_interpretation()?;
                // Reuse descriptor validation for format/interpretation compatibility.
                ImageDescriptor::new(ImageExtent::new(1, 1)?, format, interpretation, 1)?;
                Ok(ProgramBindingKind::StorageImage {
                    format,
                    access,
                    interpretation,
                })
            }
            _ => Err(unsupported(
                "only ordinary samplers and non-arrayed single-sample float 2D images are currently supported",
            )),
        },
        _ => Err(unsupported(
            "resource address space has no portable binding contract",
        )),
    }
}
fn reflect_uniform(
    module: &naga::Module,
    layouter: &naga::proc::Layouter,
    ty: naga::Handle<naga::Type>,
) -> Result<UniformLayout> {
    let TypeInner::Struct { members, span } = &module.types[ty].inner else {
        return Err(unsupported("typed uniforms require a named struct"));
    };
    if *span > PortableProgram::MAX_UNIFORM_BYTES {
        return Err(ContentError::new(
            ContentErrorKind::ResourceLimit,
            "uniform block exceeds packing budget",
        ));
    }
    let mut fields = Vec::new();
    for member in members {
        let ty = match module.types[member.ty].inner {
            TypeInner::Matrix {
                columns,
                rows,
                scalar:
                    naga::Scalar {
                        kind: ScalarKind::Float,
                        width: 4,
                    },
            } => {
                let column_alignment =
                    naga::proc::Alignment::from(rows) * naga::proc::Alignment::FOUR;
                let column_stride = column_alignment.round_up(rows as u32 * 4);
                UniformType::Matrix {
                    columns: columns as u8,
                    rows: rows as u8,
                    column_stride,
                }
            }
            _ => UniformType::Value(value_type(module, member.ty)?),
        };
        fields.push(UniformField {
            name: member
                .name
                .as_deref()
                .ok_or_else(|| shader_error("uniform member requires a name"))?
                .into(),
            offset: member.offset,
            ty,
        });
    }
    Ok(UniformLayout {
        byte_len: layouter[ty].size,
        fields,
    })
}

const FULLSCREEN_VERTEX: &str = r#"
struct _XgpuiContentVertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
@vertex fn _xgpui_content_vertex(@builtin(vertex_index) index: u32) -> _XgpuiContentVertexOut {
    let positions = array<vec2<f32>, 3>(vec2(-1.0, 1.0), vec2(3.0, 1.0), vec2(-1.0, -3.0));
    let p = positions[index];
    var out: _XgpuiContentVertexOut;
    out.position = vec4(p, 0.0, 1.0);
    out.uv = vec2((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
    return out;
}
"#;
