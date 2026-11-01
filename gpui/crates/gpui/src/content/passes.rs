//! Typed pass descriptions and CPU graph validation. No native encoders escape.
use super::graph::RecipeOperation;
use super::*;
use std::{collections::BTreeMap, ops::Range, sync::Arc};
type Result<T> = std::result::Result<T, ContentError>;
fn invalid(message: &str) -> ContentError {
    ContentError::new(ContentErrorKind::InvalidDescriptor, message)
}
fn unsupported(message: &str) -> ContentError {
    ContentError::new(ContentErrorKind::UnsupportedCapability, message)
}

/// Sampling filter in a portable texture binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FilterMode {
    /// Nearest texel.
    Nearest,
    /// Linear interpolation.
    Linear,
}
/// Texture-coordinate addressing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AddressMode {
    /// Clamp to source edge.
    Clamp,
    /// Repeat source.
    Repeat,
    /// Mirror each repeated interval.
    MirrorRepeat,
}
/// Sampling policy; enabling mip filtering requires a defined mip chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sampler {
    /// Minification filter.
    pub min_filter: FilterMode,
    /// Magnification filter.
    pub mag_filter: FilterMode,
    /// Inter-level filter; none clamps the usable LOD to level zero.
    pub mip_filter: Option<FilterMode>,
    /// Horizontal and vertical addressing.
    pub address: [AddressMode; 2],
}
impl Sampler {
    /// Linear filtering with clamped UV and no mip access.
    pub fn linear_clamp() -> Self {
        Self {
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            mip_filter: None,
            address: [AddressMode::Clamp; 2],
        }
    }
    /// Nearest filtering with clamped UV and no mip access.
    pub fn nearest_clamp() -> Self {
        Self {
            min_filter: FilterMode::Nearest,
            mag_filter: FilterMode::Nearest,
            mip_filter: None,
            address: [AddressMode::Clamp; 2],
        }
    }
    fn linear(self) -> bool {
        self.min_filter == FilterMode::Linear
            || self.mag_filter == FilterMode::Linear
            || self.mip_filter == Some(FilterMode::Linear)
    }
}

/// Executor-reported binding and vertex interface limits.
#[derive(Clone, Copy, Debug)]
pub struct BindingLimits {
    /// Maximum bind groups.
    pub max_bind_groups: u32,
    /// Maximum binding index plus one in a group.
    pub max_bindings_per_group: u32,
    /// Maximum resources used by a pass.
    pub max_bindings: usize,
    /// Maximum bytes per uniform binding.
    pub max_uniform_bytes: u64,
    /// Maximum bytes per storage binding.
    pub max_storage_bytes: u64,
    /// Native storage-binding offset alignment, in addition to WGSL layout.
    pub storage_offset_alignment: u64,
    /// Maximum vertex stream slots.
    pub max_vertex_buffers: u32,
    /// Maximum vertex attribute location plus one.
    pub max_vertex_attributes: u32,
    /// Maximum bytes per vertex/instance element.
    pub max_vertex_stride: u32,
}
impl Default for BindingLimits {
    fn default() -> Self {
        Self {
            max_bind_groups: 4,
            max_bindings_per_group: 16,
            max_bindings: 64,
            max_uniform_bytes: 64 * 1024,
            max_storage_bytes: 128 * 1024 * 1024,
            storage_offset_alignment: 256,
            max_vertex_buffers: 8,
            max_vertex_attributes: 16,
            max_vertex_stride: 2048,
        }
    }
}
/// How a vertex stream advances.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertexStep {
    /// One element per vertex.
    Vertex,
    /// One element per instance.
    Instance,
}
/// One typed attribute within a vertex/instance element.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VertexAttribute {
    /// WGSL vertex input location.
    pub location: u32,
    /// Byte offset within an element.
    pub offset: u32,
    /// Exact 32-bit scalar/vector type.
    pub ty: ShaderValueType,
}
/// Vertex stream layout validated against both its buffer and program reflection.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VertexLayout {
    /// Complete element stride, matching `BufferDescriptor::layout`.
    pub stride: u32,
    /// Per-vertex or per-instance stepping.
    pub step: VertexStep,
    /// Non-overlapping typed attributes in this stream.
    pub attributes: Vec<VertexAttribute>,
}
/// Stored index width. Index values remain a producer contract; native executors
/// must provide robust bounds protection/validation for GPU-generated indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndexFormat {
    /// Unsigned 16-bit indices.
    Uint16,
    /// Unsigned 32-bit indices.
    Uint32,
}
impl IndexFormat {
    fn width(self) -> u64 {
        match self {
            Self::Uint16 => 2,
            Self::Uint32 => 4,
        }
    }
}
/// Explicit pass-local blend operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PassBlend {
    /// Replace stored pixels.
    Replace,
    /// Source-over in a premultiplied linear color domain.
    SourceOver,
}

#[derive(Clone, Debug)]
enum BindingValue {
    Image(ImageRef),
    Sampler(Sampler),
    Uniform(UniformValues),
    Buffer(BufferSlice),
    OutputBuffer {
        descriptor: BufferDescriptor,
        initial: Option<BufferSlice>,
    },
    OutputImage {
        descriptor: ImageDescriptor,
        initial: Option<ImageRef>,
    },
}
#[derive(Clone, Debug, Default)]
struct Bindings {
    entries: BTreeMap<String, BindingValue>,
    error: Option<ContentError>,
}
impl Bindings {
    fn insert(&mut self, name: impl Into<String>, value: BindingValue) {
        if self.entries.insert(name.into(), value).is_some() {
            self.error
                .get_or_insert_with(|| invalid("duplicate named pass binding"));
        }
    }
}
macro_rules! common_bindings {
    ($ty:ident) => {
        impl $ty {
            /// Bind a sampled or read-only storage image by its reflected global name.
            pub fn image(mut self, name: impl Into<String>, image: ImageRef) -> Self {
                self.bindings.insert(name, BindingValue::Image(image));
                self
            }
            /// Bind an ordinary sampler by its reflected global name.
            pub fn sampler(mut self, name: impl Into<String>, sampler: Sampler) -> Self {
                self.bindings.insert(name, BindingValue::Sampler(sampler));
                self
            }
            /// Supply typed values to be packed against the program's uniform layout.
            pub fn uniform(mut self, name: impl Into<String>, values: UniformValues) -> Self {
                self.bindings.insert(name, BindingValue::Uniform(values));
                self
            }
            /// Bind an existing immutable storage input. Writable shader declarations
            /// require fresh compute outputs rather than this method.
            pub fn storage_buffer(mut self, name: impl Into<String>, buffer: BufferSlice) -> Self {
                self.bindings.insert(name, BindingValue::Buffer(buffer));
                self
            }
        }
    };
}
#[derive(Clone, Debug)]
struct VertexBinding {
    slice: BufferSlice,
    layout: VertexLayout,
}
#[derive(Clone, Debug)]
enum RenderGeometry {
    Fullscreen,
    Mesh {
        vertex_count: u32,
        vertices: BTreeMap<u32, VertexBinding>,
        index: Option<(BufferSlice, IndexFormat)>,
        draw: Range<u32>,
        instances: Range<u32>,
    },
}
#[derive(Clone, Debug)]
enum RenderLoad {
    Clear([u32; 4]),
    Load(ImageRef),
}
/// Immutable render intent. Construction is CPU-only; `ContentBuilder::render`
/// validates all bindings and poisons the graph attempt on an error.
#[derive(Clone, Debug)]
pub struct RenderPass {
    program: PortableProgram,
    target: ImageDescriptor,
    bindings: Bindings,
    geometry: RenderGeometry,
    load: RenderLoad,
    blend: PassBlend,
    error: Option<ContentError>,
}
common_bindings!(RenderPass);
impl RenderPass {
    /// Draw the program's generated fullscreen triangle into a defined clear target.
    pub fn fullscreen(program: &PortableProgram, target: ImageDescriptor) -> Self {
        Self::new(program, target, RenderGeometry::Fullscreen)
    }
    /// Draw triangle-list mesh geometry; default range is `0..vertex_count`, once.
    pub fn mesh(program: &PortableProgram, target: ImageDescriptor, vertex_count: u32) -> Self {
        Self::new(
            program,
            target,
            RenderGeometry::Mesh {
                vertex_count,
                vertices: BTreeMap::new(),
                index: None,
                draw: 0..vertex_count,
                instances: 0..1,
            },
        )
    }
    fn new(program: &PortableProgram, target: ImageDescriptor, geometry: RenderGeometry) -> Self {
        let alpha = if target.interpretation().alpha == AlphaMode::Opaque {
            1f32.to_bits()
        } else {
            0
        };
        Self {
            program: program.clone(),
            target,
            geometry,
            bindings: Bindings::default(),
            load: RenderLoad::Clear([0, 0, 0, alpha]),
            blend: PassBlend::Replace,
            error: None,
        }
    }
    /// Clear to a validated color in exactly the target's declared domain.
    pub fn clear(mut self, color: ColorValue) -> Self {
        if color.interpretation() != self.target.interpretation() {
            self.error = Some(invalid("clear color interpretation differs from target"));
        }
        self.load = RenderLoad::Clear(color.rgba().map(f32::to_bits));
        self
    }
    /// Copy a matching immutable source into the fresh output before rendering.
    pub fn load(mut self, source: ImageRef) -> Self {
        self.load = RenderLoad::Load(source);
        self
    }
    /// Set explicit blending. Source-over requires premultiplied linear color.
    pub fn blend(mut self, blend: PassBlend) -> Self {
        self.blend = blend;
        self
    }
    /// Bind a typed mesh stream. Repeating a slot is an error.
    pub fn vertex_buffer(mut self, slot: u32, slice: BufferSlice, layout: VertexLayout) -> Self {
        match &mut self.geometry {
            RenderGeometry::Mesh { vertices, .. } => {
                if vertices
                    .insert(slot, VertexBinding { slice, layout })
                    .is_some()
                {
                    self.error = Some(invalid("duplicate vertex buffer slot"));
                }
            }
            _ => self.error = Some(invalid("fullscreen geometry cannot bind mesh streams")),
        }
        self
    }
    /// Bind triangle-list indices and their declared element width.
    pub fn index_buffer(mut self, slice: BufferSlice, format: IndexFormat) -> Self {
        match &mut self.geometry {
            RenderGeometry::Mesh { index, .. } => {
                if index.is_some() {
                    self.error = Some(invalid("duplicate index buffer"));
                }
                *index = Some((slice, format));
            }
            _ => self.error = Some(invalid("fullscreen geometry cannot bind indices")),
        }
        self
    }
    /// Select vertex or index elements and instance range. Empty/reversed ranges
    /// are rejected; index values must be below the declared mesh vertex count.
    pub fn draw(mut self, elements: Range<u32>, instances: Range<u32>) -> Self {
        match &mut self.geometry {
            RenderGeometry::Mesh {
                draw,
                instances: current,
                ..
            } => {
                *draw = elements;
                *current = instances;
            }
            _ => self.error = Some(invalid("fullscreen geometry has a fixed draw range")),
        }
        self
    }
}
/// One dispatch with named immutable fresh outputs. Writable buffers are
/// zero-initialized and writable images are cleared in their declared domain
/// unless an explicit initial source is supplied.
#[derive(Clone, Debug)]
pub struct ComputePass {
    program: PortableProgram,
    dispatch: [u32; 3],
    bindings: Bindings,
}
common_bindings!(ComputePass);
impl ComputePass {
    /// Describe dispatch dimensions; capability validation happens in the builder.
    pub fn new(program: &PortableProgram, dispatch: [u32; 3]) -> Self {
        Self {
            program: program.clone(),
            dispatch,
            bindings: Bindings::default(),
        }
    }
    /// Bind a fresh, initialized writable storage buffer by reflected name.
    pub fn write_buffer(mut self, name: impl Into<String>, descriptor: BufferDescriptor) -> Self {
        self.bindings.insert(
            name,
            BindingValue::OutputBuffer {
                descriptor,
                initial: None,
            },
        );
        self
    }
    /// Initialize a fresh storage output from a matching immutable input slice.
    pub fn read_modify_write_buffer(
        mut self,
        name: impl Into<String>,
        descriptor: BufferDescriptor,
        source: BufferSlice,
    ) -> Self {
        self.bindings.insert(
            name,
            BindingValue::OutputBuffer {
                descriptor,
                initial: Some(source),
            },
        );
        self
    }
    /// Bind a fresh cleared storage image by reflected name.
    pub fn write_image(mut self, name: impl Into<String>, descriptor: ImageDescriptor) -> Self {
        self.bindings.insert(
            name,
            BindingValue::OutputImage {
                descriptor,
                initial: None,
            },
        );
        self
    }
    /// Initialize a fresh image output from a matching immutable image.
    pub fn read_modify_write_image(
        mut self,
        name: impl Into<String>,
        descriptor: ImageDescriptor,
        source: ImageRef,
    ) -> Self {
        self.bindings.insert(
            name,
            BindingValue::OutputImage {
                descriptor,
                initial: Some(source),
            },
        );
        self
    }
}

/// Checked immutable buffer range naming a specific graph-operation output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BufferResource {
    /// Source output selector.
    pub source: ResourceId,
    /// First byte.
    pub offset: u64,
    /// Number of accessible bytes.
    pub byte_len: u64,
}
/// Explicit initial data copied before a fresh compute output is written.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum InitialResource {
    /// Immutable image input.
    Image(ResourceId),
    /// Immutable buffer range.
    Buffer(BufferResource),
}
/// Executor-facing checked binding. Fresh outputs index the operation's output table.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BoundResource {
    /// Read-only image input.
    Image(ResourceId),
    /// Validated sampling policy.
    Sampler(Sampler),
    /// Fully initialized, reflected uniform bytes.
    Uniform(Arc<[u8]>),
    /// Read-only storage buffer input.
    Buffer(BufferResource),
    /// Fresh writable result, initialized before dispatch.
    Output {
        /// Index in `ContentNode::outputs`.
        output: usize,
        /// Optional initial immutable data; absent means defined clear/zero.
        initial: Option<InitialResource>,
    },
}
/// One validated named program binding.
#[derive(Clone, Debug)]
pub struct ResolvedBinding {
    name: Arc<str>,
    address: BindingAddress,
    resource: BoundResource,
}
impl ResolvedBinding {
    /// Reflected global name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Native binding address to lower.
    pub fn address(&self) -> BindingAddress {
        self.address
    }
    /// Checked input, uniform, sampler, or fresh output.
    pub fn resource(&self) -> &BoundResource {
        &self.resource
    }
}
/// Validated mesh input stream.
#[derive(Clone, Debug)]
pub struct ResolvedVertexBuffer {
    /// Vertex stream slot.
    pub slot: u32,
    /// Checked data range.
    pub buffer: BufferResource,
    /// Reflected-compatible layout.
    pub layout: VertexLayout,
}
/// Executor-facing fixed render geometry, after all range/type checks.
#[derive(Clone, Debug)]
pub enum ResolvedGeometry {
    /// Generated fullscreen triangle, three vertices and one instance.
    Fullscreen,
    /// Triangle-list mesh. GPU-generated index values require native robust validation.
    Mesh {
        /// Declared maximum accessible vertices.
        vertex_count: u32,
        /// Validated input streams.
        vertices: Vec<ResolvedVertexBuffer>,
        /// Optional typed index range.
        index: Option<(BufferResource, IndexFormat)>,
        /// Vertex or index draw range.
        elements: Range<u32>,
        /// Instance range.
        instances: Range<u32>,
    },
}
/// Defined initial render-target contents.
#[derive(Clone, Debug)]
pub enum ResolvedLoad {
    /// Finite channel bits in the target's color/alpha domain.
    Clear([u32; 4]),
    /// Matching immutable image to copy before the pass.
    Load(ResourceId),
}
/// Fully validated render operation, ready for backend lowering.
#[derive(Clone, Debug)]
pub struct ResolvedRenderPass {
    program: PortableProgram,
    bindings: Vec<ResolvedBinding>,
    geometry: ResolvedGeometry,
    load: ResolvedLoad,
    blend: PassBlend,
}
/// Fully validated single compute dispatch and all named outputs.
#[derive(Clone, Debug)]
pub struct ResolvedComputePass {
    program: PortableProgram,
    bindings: Vec<ResolvedBinding>,
    dispatch: [u32; 3],
}
impl ResolvedRenderPass {
    /// Validated shader artifact.
    pub fn program(&self) -> &PortableProgram {
        &self.program
    }
    /// Checked bindings in reflected address order.
    pub fn bindings(&self) -> &[ResolvedBinding] {
        &self.bindings
    }
    /// Checked geometry and draw ranges.
    pub fn geometry(&self) -> &ResolvedGeometry {
        &self.geometry
    }
    /// Defined render load operation.
    pub fn load(&self) -> &ResolvedLoad {
        &self.load
    }
    /// Explicit blend state.
    pub fn blend(&self) -> PassBlend {
        self.blend
    }
    pub(super) fn payload_bytes(&self) -> u64 {
        uniform_bytes(&self.bindings)
    }
    pub(super) fn remap(&mut self, map: &[usize]) {
        remap_bindings(&mut self.bindings, map);
        if let ResolvedLoad::Load(id) = &mut self.load {
            id.node = map[id.node];
        }
        if let ResolvedGeometry::Mesh {
            vertices, index, ..
        } = &mut self.geometry
        {
            for vertex in vertices {
                vertex.buffer.source.node = map[vertex.buffer.source.node];
            }
            if let Some((buffer, _)) = index {
                buffer.source.node = map[buffer.source.node];
            }
        }
    }
}
impl ResolvedComputePass {
    /// Validated shader artifact.
    pub fn program(&self) -> &PortableProgram {
        &self.program
    }
    /// Checked bindings, including named fresh outputs.
    pub fn bindings(&self) -> &[ResolvedBinding] {
        &self.bindings
    }
    /// Workgroups per axis.
    pub fn dispatch(&self) -> [u32; 3] {
        self.dispatch
    }
    pub(super) fn payload_bytes(&self) -> u64 {
        uniform_bytes(&self.bindings)
    }
    pub(super) fn remap(&mut self, map: &[usize]) {
        remap_bindings(&mut self.bindings, map);
    }
}
fn uniform_bytes(bindings: &[ResolvedBinding]) -> u64 {
    bindings
        .iter()
        .map(|b| match &b.resource {
            BoundResource::Uniform(bytes) => bytes.len() as u64,
            _ => 0,
        })
        .sum()
}
fn remap_bindings(bindings: &mut [ResolvedBinding], map: &[usize]) {
    for binding in bindings {
        match &mut binding.resource {
            BoundResource::Image(id) => id.node = map[id.node],
            BoundResource::Buffer(buffer) => buffer.source.node = map[buffer.source.node],
            BoundResource::Output {
                initial: Some(InitialResource::Image(id)),
                ..
            } => id.node = map[id.node],
            BoundResource::Output {
                initial: Some(InitialResource::Buffer(buffer)),
                ..
            } => buffer.source.node = map[buffer.source.node],
            _ => {}
        }
    }
}
#[derive(Clone, Debug)]
enum ComputeRef {
    Image(ImageRef),
    Buffer(BufferRef),
}
/// Named outputs of one dispatch. Looking up an output never records another dispatch.
#[derive(Clone, Debug)]
pub struct ComputeOutputs(BTreeMap<String, ComputeRef>);
impl ComputeOutputs {
    /// Retrieve a named image result; errors on unknown names or buffer outputs.
    pub fn image(&self, name: &str) -> Result<ImageRef> {
        match self.0.get(name) {
            Some(ComputeRef::Image(image)) => Ok(*image),
            _ => Err(invalid("unknown or non-image compute output")),
        }
    }
    /// Retrieve a named buffer result; errors on unknown names or image outputs.
    pub fn buffer(&self, name: &str) -> Result<BufferRef> {
        match self.0.get(name) {
            Some(ComputeRef::Buffer(buffer)) => Ok(*buffer),
            _ => Err(invalid("unknown or non-buffer compute output")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum KeyBinding {
    Image(usize),
    Sampler(Sampler),
    Uniform(Arc<[u8]>),
    Buffer {
        input: usize,
        offset: u64,
        byte_len: u64,
    },
    Output {
        output: usize,
        initial: Option<KeyInitial>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum KeyInitial {
    Image(usize),
    Buffer {
        input: usize,
        offset: u64,
        byte_len: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct KeyVertex {
    slot: u32,
    input: usize,
    offset: u64,
    byte_len: u64,
    layout: VertexLayout,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum KeyGeometry {
    Fullscreen,
    Mesh {
        vertex_count: u32,
        vertices: Vec<KeyVertex>,
        index: Option<(usize, u64, u64, IndexFormat)>,
        elements: Range<u32>,
        instances: Range<u32>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum KeyLoad {
    Clear([u32; 4]),
    Load(usize),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PassKindKey {
    Render {
        geometry: KeyGeometry,
        load: KeyLoad,
        blend: PassBlend,
    },
    Compute([u32; 3]),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct PassKey {
    program: PortableProgram,
    bindings: Vec<KeyBinding>,
    kind: PassKindKey,
}

#[derive(Default)]
struct Inputs(Vec<ResourceId>);
impl Inputs {
    fn add(&mut self, id: ResourceId) -> usize {
        if let Some(index) = self.0.iter().position(|candidate| *candidate == id) {
            index
        } else {
            let index = self.0.len();
            self.0.push(id);
            index
        }
    }
    fn buffer_key(&mut self, b: BufferResource) -> (usize, u64, u64) {
        (self.add(b.source), b.offset, b.byte_len)
    }
}
fn binding_keys(bindings: &[ResolvedBinding], inputs: &mut Inputs) -> Vec<KeyBinding> {
    bindings
        .iter()
        .map(|b| match &b.resource {
            BoundResource::Image(id) => KeyBinding::Image(inputs.add(*id)),
            BoundResource::Sampler(sampler) => KeyBinding::Sampler(*sampler),
            BoundResource::Uniform(bytes) => KeyBinding::Uniform(bytes.clone()),
            BoundResource::Buffer(b) => {
                let (input, offset, byte_len) = inputs.buffer_key(*b);
                KeyBinding::Buffer {
                    input,
                    offset,
                    byte_len,
                }
            }
            BoundResource::Output { output, initial } => KeyBinding::Output {
                output: *output,
                initial: initial.as_ref().map(|initial| match initial {
                    InitialResource::Image(id) => KeyInitial::Image(inputs.add(*id)),
                    InitialResource::Buffer(b) => {
                        let (input, offset, byte_len) = inputs.buffer_key(*b);
                        KeyInitial::Buffer {
                            input,
                            offset,
                            byte_len,
                        }
                    }
                }),
            },
        })
        .collect()
}

impl ContentBuilder {
    /// Validate and record one render pass. Any returned error poisons this graph.
    pub fn render(&mut self, pass: RenderPass) -> Result<ImageRef> {
        self.healthy()?;
        let result = self.render_checked(pass);
        self.checked(result)
    }
    fn render_checked(&mut self, pass: RenderPass) -> Result<ImageRef> {
        if let Some(error) = pass.error {
            return Err(error);
        }
        if pass.program.render_entries().is_none()
            || pass.program.fragment_output() != Some(pass.target.interpretation())
        {
            return Err(invalid(
                "render program and target interpretation must match",
            ));
        }
        if pass.target.mip_levels() != 1 {
            return Err(invalid(
                "render targets define one mip; generate additional mips explicitly",
            ));
        }
        if !self
            .capabilities()
            .renderable_image_formats
            .contains(&pass.target.format())
        {
            return Err(unsupported("render target format is unavailable"));
        }
        if pass.blend == PassBlend::SourceOver
            && (pass.target.interpretation().alpha != AlphaMode::Premultiplied
                || !matches!(
                    pass.target.interpretation().color,
                    ColorEncoding::LinearSrgb
                        | ColorEncoding::LinearP3
                        | ColorEncoding::LinearRec2020
                ))
        {
            return Err(invalid("source-over requires premultiplied linear color"));
        }
        let mut outputs = Vec::new();
        let bindings = self.resolve_bindings(&pass.program, pass.bindings, false, &mut outputs)?;
        let mut inputs = Inputs::default();
        let keys = binding_keys(&bindings, &mut inputs);
        let (geometry, geometry_key) =
            self.resolve_geometry(&pass.program, pass.geometry, &mut inputs)?;
        let (load, load_key) = match pass.load {
            RenderLoad::Clear(bits) => (ResolvedLoad::Clear(bits), KeyLoad::Clear(bits)),
            RenderLoad::Load(image) => {
                if self.image_descriptor(image)? != &pass.target {
                    return Err(invalid("render load source descriptor differs from output"));
                }
                let id = self.image_node(image)?;
                (ResolvedLoad::Load(id), KeyLoad::Load(inputs.add(id)))
            }
        };
        let resolved = ResolvedRenderPass {
            program: pass.program.clone(),
            bindings,
            geometry,
            load,
            blend: pass.blend,
        };
        let payload = resolved.payload_bytes();
        let key = PassKey {
            program: pass.program,
            bindings: keys,
            kind: PassKindKey::Render {
                geometry: geometry_key,
                load: load_key,
                blend: pass.blend,
            },
        };
        let node = self.add_outputs(
            vec![ResourceDescriptor::Image(pass.target)],
            ContentOperation::Render(resolved),
            RecipeOperation::Pass(key),
            inputs.0,
            payload,
        )?;
        Ok(ImageRef {
            graph: self.graph_identity(),
            node,
            output: 0,
        })
    }
    /// Validate a single dispatch and atomically declare all fresh outputs.
    pub fn compute(&mut self, pass: ComputePass) -> Result<ComputeOutputs> {
        self.healthy()?;
        let result = self.compute_checked(pass);
        self.checked(result)
    }
    fn compute_checked(&mut self, pass: ComputePass) -> Result<ComputeOutputs> {
        pass.program
            .validate_dispatch(pass.dispatch, self.capabilities().compute.as_ref())?;
        let mut outputs = Vec::new();
        let bindings = self.resolve_bindings(&pass.program, pass.bindings, true, &mut outputs)?;
        if outputs.is_empty() {
            return Err(invalid(
                "compute passes must declare a fresh writable output",
            ));
        }
        let names: Vec<_> = bindings
            .iter()
            .filter_map(|b| match b.resource {
                BoundResource::Output { output, .. } => Some((b.name.to_string(), output)),
                _ => None,
            })
            .collect();
        let mut inputs = Inputs::default();
        let keys = binding_keys(&bindings, &mut inputs);
        let resolved = ResolvedComputePass {
            program: pass.program.clone(),
            bindings,
            dispatch: pass.dispatch,
        };
        let payload = resolved.payload_bytes();
        let key = PassKey {
            program: pass.program,
            bindings: keys,
            kind: PassKindKey::Compute(pass.dispatch),
        };
        let kinds: Vec<_> = outputs
            .iter()
            .map(|d| matches!(d, ResourceDescriptor::Image(_)))
            .collect();
        let node = self.add_outputs(
            outputs,
            ContentOperation::Compute(resolved),
            RecipeOperation::Pass(key),
            inputs.0,
            payload,
        )?;
        let graph = self.graph_identity();
        Ok(ComputeOutputs(
            names
                .into_iter()
                .map(|(name, output)| {
                    (
                        name,
                        if kinds[output] {
                            ComputeRef::Image(ImageRef {
                                graph,
                                node,
                                output,
                            })
                        } else {
                            ComputeRef::Buffer(BufferRef {
                                graph,
                                node,
                                output,
                            })
                        },
                    )
                })
                .collect(),
        ))
    }
    fn buffer_resource(
        &mut self,
        slice: BufferSlice,
        usage: BufferUsage,
    ) -> Result<BufferResource> {
        self.validate_buffer_slice(slice, usage)?;
        Ok(BufferResource {
            source: self.buffer_node(slice.buffer)?,
            offset: slice.offset,
            byte_len: slice.byte_len,
        })
    }
    fn validate_storage_buffer(
        &mut self,
        slice: BufferSlice,
        layout: &StorageBufferLayout,
    ) -> Result<BufferResource> {
        let b = self.buffer_resource(slice, BufferUsage::STORAGE)?;
        let limits = self.capabilities().bindings;
        if limits.storage_offset_alignment == 0
            || !b.offset.is_multiple_of(limits.storage_offset_alignment)
            || b.byte_len > limits.max_storage_bytes
        {
            return Err(unsupported("storage binding range exceeds executor limits"));
        }
        if !b.offset.is_multiple_of(u64::from(layout.alignment())) {
            return Err(invalid("storage binding offset violates shader alignment"));
        }
        layout.validate_byte_len(b.byte_len)?;
        Ok(b)
    }
    fn resolve_bindings(
        &mut self,
        program: &PortableProgram,
        values: Bindings,
        compute: bool,
        outputs: &mut Vec<ResourceDescriptor>,
    ) -> Result<Vec<ResolvedBinding>> {
        if let Some(error) = values.error {
            return Err(error);
        }
        let limits = self.capabilities().bindings;
        if program.bindings().len() > limits.max_bindings
            || program.bindings().iter().any(|b| {
                b.address().group >= limits.max_bind_groups
                    || b.address().binding >= limits.max_bindings_per_group
            })
        {
            return Err(unsupported(
                "program binding interface exceeds executor limits",
            ));
        }
        if values.entries.len() != program.bindings().len()
            || values
                .entries
                .keys()
                .any(|name| program.binding(name).is_none())
        {
            return Err(invalid(
                "pass bindings must match exactly the reflected resources",
            ));
        }
        let mut bindings = Vec::new();
        for binding in program.bindings() {
            let value = values
                .entries
                .get(binding.name())
                .ok_or_else(|| invalid("missing pass binding"))?;
            let resource = match (binding.kind(), value) {
                (ProgramBindingKind::Uniform(layout), BindingValue::Uniform(values)) => {
                    if u64::from(layout.byte_len()) > limits.max_uniform_bytes {
                        return Err(unsupported("uniform binding exceeds executor limit"));
                    }
                    BoundResource::Uniform(layout.pack(values)?)
                }
                (ProgramBindingKind::Sampler, BindingValue::Sampler(sampler)) => {
                    BoundResource::Sampler(*sampler)
                }
                (ProgramBindingKind::SampledImage(interpretation), BindingValue::Image(image)) => {
                    if self.image_descriptor(*image)?.interpretation() != *interpretation {
                        return Err(invalid(
                            "sampled image interpretation differs from program contract; convert explicitly",
                        ));
                    }
                    BoundResource::Image(self.image_node(*image)?)
                }
                (
                    ProgramBindingKind::StorageImage {
                        format,
                        access: StorageAccess::ReadOnly,
                        interpretation,
                    },
                    BindingValue::Image(image),
                ) => {
                    let d = self.image_descriptor(*image)?.clone();
                    self.validate_storage_image(&d, *format, *interpretation)?;
                    BoundResource::Image(self.image_node(*image)?)
                }
                (
                    ProgramBindingKind::StorageBuffer {
                        layout,
                        access: StorageAccess::ReadOnly,
                    },
                    BindingValue::Buffer(buffer),
                ) => BoundResource::Buffer(self.validate_storage_buffer(*buffer, layout)?),
                (
                    ProgramBindingKind::StorageBuffer { layout, access },
                    BindingValue::OutputBuffer {
                        descriptor,
                        initial,
                    },
                ) if compute && *access != StorageAccess::ReadOnly => {
                    if !descriptor.usage().contains(BufferUsage::STORAGE) {
                        return Err(invalid("compute buffer outputs require STORAGE usage"));
                    }
                    layout.validate_byte_len(descriptor.byte_len())?;
                    if descriptor.byte_len() > limits.max_storage_bytes {
                        return Err(unsupported("storage output exceeds executor binding limit"));
                    }
                    let initial = if let Some(slice) = initial {
                        let d = self.buffer_descriptor(slice.buffer)?.clone();
                        if d.layout() != descriptor.layout()
                            || slice.byte_len != descriptor.byte_len()
                        {
                            return Err(invalid(
                                "compute initial buffer layout/size differs from output",
                            ));
                        }
                        Some(InitialResource::Buffer(
                            self.buffer_resource(*slice, BufferUsage::COPY_SOURCE)?,
                        ))
                    } else {
                        None
                    };
                    let output = outputs.len();
                    outputs.push(ResourceDescriptor::Buffer(descriptor.clone()));
                    BoundResource::Output { output, initial }
                }
                (
                    ProgramBindingKind::StorageImage {
                        format,
                        access,
                        interpretation,
                    },
                    BindingValue::OutputImage {
                        descriptor,
                        initial,
                    },
                ) if compute && *access != StorageAccess::ReadOnly => {
                    self.validate_storage_image(descriptor, *format, *interpretation)?;
                    let initial = if let Some(image) = initial {
                        if self.image_descriptor(*image)? != descriptor {
                            return Err(invalid(
                                "compute initial image differs from output descriptor",
                            ));
                        }
                        Some(InitialResource::Image(self.image_node(*image)?))
                    } else {
                        None
                    };
                    let output = outputs.len();
                    outputs.push(ResourceDescriptor::Image(descriptor.clone()));
                    BoundResource::Output { output, initial }
                }
                _ => {
                    return Err(invalid(
                        "pass binding type/access differs from reflected resource; writes require fresh outputs",
                    ));
                }
            };
            bindings.push(ResolvedBinding {
                name: binding.name().into(),
                address: binding.address(),
                resource,
            });
        }
        for pair in program.sampling_pairs() {
            let BindingValue::Image(image) = values.entries.get(pair.image.as_ref()).unwrap()
            else {
                unreachable!()
            };
            let BindingValue::Sampler(sampler) = values.entries.get(pair.sampler.as_ref()).unwrap()
            else {
                unreachable!()
            };
            let d = self.image_descriptor(*image)?.clone();
            if sampler.linear()
                && !self
                    .capabilities()
                    .filterable_image_formats
                    .contains(&d.format())
            {
                return Err(unsupported(
                    "image format does not support requested linear filtering",
                ));
            }
            if sampler.mip_filter.is_some()
                && d.mip_levels() <= 1
                && (d.extent().width() > 1 || d.extent().height() > 1)
            {
                return Err(invalid(
                    "mip filtering requires explicitly defined mip levels",
                ));
            }
        }
        Ok(bindings)
    }
    fn validate_storage_image(
        &self,
        descriptor: &ImageDescriptor,
        format: PixelFormat,
        interpretation: ImageInterpretation,
    ) -> Result<()> {
        if descriptor.format() != format
            || descriptor.interpretation() != interpretation
            || descriptor.mip_levels() != 1
        {
            return Err(invalid(
                "storage image format, interpretation, or mip count differs from shader contract",
            ));
        }
        if !self.capabilities().storage_image_formats.contains(&format) {
            return Err(unsupported("storage image format is unavailable"));
        }
        Ok(())
    }
    fn resolve_geometry(
        &mut self,
        program: &PortableProgram,
        geometry: RenderGeometry,
        inputs: &mut Inputs,
    ) -> Result<(ResolvedGeometry, KeyGeometry)> {
        match geometry {
            RenderGeometry::Fullscreen => {
                if !program.is_fullscreen() {
                    return Err(invalid(
                        "fullscreen passes require a generated fullscreen program",
                    ));
                }
                Ok((ResolvedGeometry::Fullscreen, KeyGeometry::Fullscreen))
            }
            RenderGeometry::Mesh {
                vertex_count,
                vertices,
                index,
                draw,
                instances,
            } => {
                if program.is_fullscreen()
                    || vertex_count == 0
                    || draw.start >= draw.end
                    || instances.start >= instances.end
                    || (draw.end - draw.start) % 3 != 0
                {
                    return Err(invalid(
                        "mesh requires a mesh program, nonempty ranges and complete triangles",
                    ));
                }
                let limits = self.capabilities().bindings;
                let mut attributes = BTreeMap::new();
                let mut resolved = Vec::new();
                let mut keys = Vec::new();
                for (slot, mut vertex) in vertices {
                    vertex
                        .layout
                        .attributes
                        .sort_by_key(|attribute| attribute.location);
                    if slot >= limits.max_vertex_buffers
                        || vertex.layout.stride > limits.max_vertex_stride
                    {
                        return Err(unsupported("vertex stream exceeds executor limits"));
                    }
                    let d = self.buffer_descriptor(vertex.slice.buffer)?.clone();
                    if vertex.layout.stride == 0
                        || vertex.layout.stride != d.layout().stride()
                        || !vertex.layout.stride.is_multiple_of(4)
                        || vertex.layout.attributes.is_empty()
                    {
                        return Err(invalid(
                            "vertex layout stride differs from buffer or has no attributes",
                        ));
                    }
                    let mut ranges = Vec::new();
                    for attribute in &vertex.layout.attributes {
                        let lanes = match attribute.ty {
                            ShaderValueType::Float(n)
                            | ShaderValueType::Sint(n)
                            | ShaderValueType::Uint(n) => n,
                        };
                        let end = attribute
                            .offset
                            .checked_add(u32::from(lanes) * 4)
                            .ok_or_else(|| invalid("attribute offset overflow"))?;
                        if !(1..=4).contains(&lanes)
                            || !attribute.offset.is_multiple_of(4)
                            || end > vertex.layout.stride
                            || ranges
                                .iter()
                                .any(|(start, old_end)| attribute.offset < *old_end && *start < end)
                            || attributes
                                .insert(attribute.location, attribute.ty)
                                .is_some()
                        {
                            return Err(invalid(
                                "vertex attributes overlap, duplicate locations or exceed stride",
                            ));
                        }
                        if attribute.location >= limits.max_vertex_attributes {
                            return Err(unsupported("vertex attribute exceeds executor limits"));
                        }
                        ranges.push((attribute.offset, end));
                    }
                    let count = if vertex.layout.step == VertexStep::Vertex {
                        vertex_count
                    } else {
                        instances.end
                    };
                    let required = u64::from(count) * u64::from(vertex.layout.stride);
                    if vertex.slice.byte_len < required {
                        return Err(invalid(
                            "vertex/instance stream is shorter than declared draw range",
                        ));
                    }
                    let buffer = self.buffer_resource(vertex.slice, BufferUsage::VERTEX)?;
                    keys.push(KeyVertex {
                        slot,
                        input: inputs.add(buffer.source),
                        offset: buffer.offset,
                        byte_len: buffer.byte_len,
                        layout: vertex.layout.clone(),
                    });
                    resolved.push(ResolvedVertexBuffer {
                        slot,
                        buffer,
                        layout: vertex.layout,
                    });
                }
                let expected: BTreeMap<_, _> = program
                    .vertex_inputs()
                    .iter()
                    .map(|input| (input.location, input.ty))
                    .collect();
                if attributes != expected {
                    return Err(invalid(
                        "vertex attributes differ from the reflected shader interface",
                    ));
                }
                let index = if let Some((slice, format)) = index {
                    let d = self.buffer_descriptor(slice.buffer)?.clone();
                    let buffer = self.buffer_resource(slice, BufferUsage::INDEX)?;
                    if u64::from(d.layout().stride()) != format.width()
                        || !buffer.offset.is_multiple_of(format.width())
                        || u64::from(draw.end) > buffer.byte_len / format.width()
                    {
                        return Err(invalid(
                            "index format or draw range exceeds the bound index elements",
                        ));
                    }
                    Some((buffer, format))
                } else {
                    if draw.end > vertex_count {
                        return Err(invalid("vertex draw range exceeds declared vertex count"));
                    }
                    None
                };
                let key_index = index.map(|(b, format)| {
                    let (input, offset, byte_len) = inputs.buffer_key(b);
                    (input, offset, byte_len, format)
                });
                Ok((
                    ResolvedGeometry::Mesh {
                        vertex_count,
                        vertices: resolved,
                        index,
                        elements: draw.clone(),
                        instances: instances.clone(),
                    },
                    KeyGeometry::Mesh {
                        vertex_count,
                        vertices: keys,
                        index: key_index,
                        elements: draw,
                        instances,
                    },
                ))
            }
        }
    }
}
