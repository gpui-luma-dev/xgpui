use super::*;
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    sync::Arc,
};

type Result<T> = std::result::Result<T, ContentError>;
fn error(kind: ContentErrorKind, message: &str) -> ContentError {
    ContentError::new(kind, message)
}

/// CPU-only immutable producer. Implementations must not perform GPU work or waits.
pub trait ContentSource: Debug + Send + Sync + 'static {
    /// Declare an immutable result and its dependencies in the supplied builder.
    fn describe(&self, cx: &mut ContentBuilder, request: &ContentRequest) -> Result<ImageRef>;
}

/// Retained immutable description identity; cloning preserves the source instance.
#[derive(Clone, Debug)]
pub struct Content {
    id: u64,
    source: Arc<dyn ContentSource>,
    label: Option<Arc<str>>,
}
impl Content {
    /// Assign a unique instance identity. Equal pure operations can still share results.
    pub fn new(source: impl ContentSource) -> Self {
        Self {
            id: next_content_identity(),
            source: Arc::new(source),
            label: None,
        }
    }
    /// Built-in structurally reusable solid, safe to reconstruct each render.
    pub fn solid(color: ColorValue) -> Self {
        Self::new(Solid(color))
    }
    /// Attach a diagnostic label without changing content or realization identity.
    pub fn labeled(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }
    /// Immutable instance identity, not a user-supplied cache key.
    pub fn id(&self) -> u64 {
        self.id
    }
    /// Optional diagnostic label.
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }
}
#[derive(Debug)]
struct Solid(ColorValue);
impl ContentSource for Solid {
    fn describe(&self, cx: &mut ContentBuilder, request: &ContentRequest) -> Result<ImageRef> {
        cx.solid(request.extent(), self.0.clone())
    }
}

/// A temporary image output, valid only in the builder that issued it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageRef {
    graph: u64,
    node: usize,
}
/// A temporary buffer output, valid only in the builder that issued it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BufferRef {
    graph: u64,
    node: usize,
}
/// An immutable, explicitly bounded part of a buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferSlice {
    /// Builder-local buffer reference.
    pub buffer: BufferRef,
    /// Start byte offset.
    pub offset: u64,
    /// Number of whole layout elements.
    pub byte_len: u64,
}

/// Resource limits supplied by an executor; not backend names or queue capacities.
#[derive(Clone, Debug)]
pub struct ContentCapabilities {
    /// Largest supported image dimension.
    pub max_image_dimension: u32,
    /// Largest supported single buffer.
    pub max_buffer_bytes: u64,
    /// Supported internal texture formats (separate from presentation support).
    pub image_formats: Vec<PixelFormat>,
}
impl Default for ContentCapabilities {
    fn default() -> Self {
        Self {
            max_image_dimension: 8192,
            max_buffer_bytes: 256 * 1024 * 1024,
            image_formats: vec![
                PixelFormat::Rgba8,
                PixelFormat::Bgra8,
                PixelFormat::Rgba16Float,
                PixelFormat::R8,
            ],
        }
    }
}
/// Explicit CPU description resource policy, checked before accepting operations.
#[derive(Clone, Copy, Debug)]
pub struct GraphLimits {
    /// Maximum unique operations.
    pub max_nodes: usize,
    /// Maximum immutable upload bytes retained by unique operations.
    pub max_payload_bytes: u64,
    /// Maximum nested producer calls and operation dependency depth.
    pub max_depth: usize,
}
impl Default for GraphLimits {
    fn default() -> Self {
        Self {
            max_nodes: 65536,
            max_payload_bytes: 256 * 1024 * 1024,
            max_depth: 128,
        }
    }
}

/// One requested root; source output extent is independent of destination extent.
#[derive(Clone, Debug)]
pub struct ContentRoot {
    /// Immutable source.
    pub content: Content,
    /// Full source geometry request.
    pub request: ContentRequest,
}

/// Typed descriptor of one logical immutable resource version.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ResourceDescriptor {
    /// Image storage and interpretation.
    Image(ImageDescriptor),
    /// Typed buffer storage.
    Buffer(BufferDescriptor),
}
impl ResourceDescriptor {
    /// Unpadded allocation size; executors must additionally account for native padding.
    pub fn byte_len(&self) -> u64 {
        match self {
            Self::Image(d) => d.byte_len(),
            Self::Buffer(d) => d.byte_len(),
        }
    }
}

/// CPU operation description. Indices name earlier operations in the same graph.
#[derive(Clone, Debug)]
pub enum ContentOperation {
    /// Fill a logical image; direct lowering is optional, not implied.
    Solid(ColorValue),
    /// Immutable, tightly packed mip payload, largest level first.
    UploadImage(Arc<[u8]>),
    /// Copy a whole image with unchanged interpretation and layout.
    CopyImage {
        /// Source node index.
        source: usize,
    },
    /// Immutable typed buffer upload.
    UploadBuffer(Arc<[u8]>),
    /// Defined zero-filled buffer without allocating the CPU payload.
    ZeroBuffer,
    /// Copy a checked source range into a fresh immutable buffer.
    CopyBuffer {
        /// Source node index.
        source: usize,
        /// First source byte.
        offset: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum RecipeOperation {
    Solid(ColorValue),
    UploadImage(Arc<[u8]>),
    CopyImage,
    UploadBuffer(Arc<[u8]>),
    ZeroBuffer,
    CopyBuffer { offset: u64 },
}
#[derive(Debug, PartialEq, Eq, Hash)]
struct Recipe {
    descriptor: ResourceDescriptor,
    operation: RecipeOperation,
    inputs: Vec<RealizationKey>,
}
/// Collision-checked structural realization key; includes owned dependency recipes.
/// Device generation and executor policy must be added by a device cache.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RealizationKey(Arc<Recipe>);

/// A validated operation with immutable payload and explicit resource dependencies.
#[derive(Clone, Debug)]
pub struct ContentNode {
    descriptor: ResourceDescriptor,
    operation: ContentOperation,
    dependencies: Vec<usize>,
    key: RealizationKey,
    depth: usize,
}
impl ContentNode {
    /// Logical result storage and interpretation.
    pub fn descriptor(&self) -> &ResourceDescriptor {
        &self.descriptor
    }
    /// CPU operation to lower on the executor.
    pub fn operation(&self) -> &ContentOperation {
        &self.operation
    }
    /// Earlier node indices, including all resource reads.
    pub fn dependencies(&self) -> &[usize] {
        &self.dependencies
    }
    /// Portable structural identity; labels and native handles are excluded.
    pub fn realization_key(&self) -> &RealizationKey {
        &self.key
    }
}

/// Validated CPU graph. Creation performs no native allocation or submission.
#[derive(Debug)]
pub struct ContentGraph {
    nodes: Vec<ContentNode>,
    roots: Vec<usize>,
    payload_bytes: u64,
}
impl ContentGraph {
    /// Renderer-facing description entry point. Unreachable operations are pruned.
    pub fn describe(
        roots: &[ContentRoot],
        capabilities: ContentCapabilities,
        limits: GraphLimits,
    ) -> Result<Self> {
        if limits.max_depth == 0 || limits.max_nodes == 0 {
            return Err(error(
                ContentErrorKind::ResourceLimit,
                "graph depth and node limits must be positive",
            ));
        }
        let mut builder = ContentBuilder {
            id: next_content_identity(),
            capabilities,
            limits,
            nodes: Vec::new(),
            keys: HashMap::new(),
            described: HashMap::new(),
            active: HashSet::new(),
            poison: None,
            payload_bytes: 0,
        };
        let mut outputs = Vec::new();
        for root in roots {
            outputs.push(builder.source(&root.content, &root.request)?.node);
        }
        builder.healthy()?;
        let mut needed = vec![false; builder.nodes.len()];
        let mut stack = outputs.clone();
        while let Some(id) = stack.pop() {
            if !needed[id] {
                needed[id] = true;
                stack.extend(builder.nodes[id].dependencies.iter().copied());
            }
        }
        let mut remap = vec![0; builder.nodes.len()];
        let mut nodes = Vec::new();
        let mut payload_bytes = 0;
        for (old, mut node) in builder.nodes.into_iter().enumerate() {
            if !needed[old] {
                continue;
            }
            remap[old] = nodes.len();
            for dependency in &mut node.dependencies {
                *dependency = remap[*dependency];
            }
            match &mut node.operation {
                ContentOperation::CopyImage { source }
                | ContentOperation::CopyBuffer { source, .. } => *source = remap[*source],
                ContentOperation::UploadImage(bytes) | ContentOperation::UploadBuffer(bytes) => {
                    payload_bytes += bytes.len() as u64
                }
                _ => {}
            }
            nodes.push(node);
        }
        Ok(Self {
            nodes,
            roots: outputs.into_iter().map(|id| remap[id]).collect(),
            payload_bytes,
        })
    }
    /// Operations in dependency-first order.
    pub fn nodes(&self) -> &[ContentNode] {
        &self.nodes
    }
    /// One output node per requested root, including duplicate uses.
    pub fn roots(&self) -> &[usize] {
        &self.roots
    }
    /// Retained upload payload bytes, counted once per unique operation.
    pub fn payload_bytes(&self) -> u64 {
        self.payload_bytes
    }
}

/// Host-created CPU description builder. Native recording is intentionally absent.
pub struct ContentBuilder {
    id: u64,
    capabilities: ContentCapabilities,
    limits: GraphLimits,
    nodes: Vec<ContentNode>,
    keys: HashMap<RealizationKey, usize>,
    described: HashMap<(u64, ContentRequest), ImageRef>,
    active: HashSet<u64>,
    poison: Option<ContentError>,
    payload_bytes: u64,
}
impl ContentBuilder {
    fn healthy(&self) -> Result<()> {
        self.poison.clone().map_or(Ok(()), Err)
    }
    fn checked<T>(&mut self, result: Result<T>) -> Result<T> {
        if let Err(error) = &result {
            self.poison.get_or_insert_with(|| error.clone());
        }
        result
    }
    /// Probe executor functionality before choosing an operation; no side effects.
    pub fn capabilities(&self) -> &ContentCapabilities {
        &self.capabilities
    }
    /// Describe a nested immutable source; reject recursion and foreign output refs.
    pub fn source(&mut self, content: &Content, request: &ContentRequest) -> Result<ImageRef> {
        self.healthy()?;
        let key = (content.id, request.clone());
        if let Some(result) = self.described.get(&key) {
            return Ok(*result);
        }
        if self.active.contains(&content.id) {
            return self.checked(Err(error(
                ContentErrorKind::Cycle,
                "recursive content dependency",
            )));
        }
        if self.active.len() >= self.limits.max_depth {
            return self.checked(Err(error(
                ContentErrorKind::ResourceLimit,
                "producer description depth exceeded",
            )));
        }
        self.active.insert(content.id);
        let result = content.source.describe(self, request);
        self.active.remove(&content.id);
        let result = self.checked(result)?;
        self.healthy()?;
        self.image_node(result)?;
        self.described.insert(key, result);
        Ok(result)
    }
    fn image_node(&mut self, image: ImageRef) -> Result<usize> {
        self.healthy()?;
        let valid = image.graph == self.id
            && self
                .nodes
                .get(image.node)
                .is_some_and(|n| matches!(n.descriptor, ResourceDescriptor::Image(_)));
        self.checked(if valid {
            Ok(image.node)
        } else {
            Err(error(
                ContentErrorKind::InvalidReference,
                "foreign or invalid image reference",
            ))
        })
    }
    fn buffer_node(&mut self, buffer: BufferRef) -> Result<usize> {
        self.healthy()?;
        let valid = buffer.graph == self.id
            && self
                .nodes
                .get(buffer.node)
                .is_some_and(|n| matches!(n.descriptor, ResourceDescriptor::Buffer(_)));
        self.checked(if valid {
            Ok(buffer.node)
        } else {
            Err(error(
                ContentErrorKind::InvalidReference,
                "foreign or invalid buffer reference",
            ))
        })
    }
    fn add(
        &mut self,
        descriptor: ResourceDescriptor,
        operation: ContentOperation,
        recipe: RecipeOperation,
        dependencies: Vec<usize>,
        payload: u64,
    ) -> Result<usize> {
        self.healthy()?;
        let supported = match &descriptor {
            ResourceDescriptor::Image(d) => {
                d.extent().width() <= self.capabilities.max_image_dimension
                    && d.extent().height() <= self.capabilities.max_image_dimension
                    && self.capabilities.image_formats.contains(&d.format())
            }
            ResourceDescriptor::Buffer(d) => d.byte_len() <= self.capabilities.max_buffer_bytes,
        };
        if !supported {
            return self.checked(Err(error(
                ContentErrorKind::UnsupportedCapability,
                "resource exceeds executor capabilities",
            )));
        }
        let key = RealizationKey(Arc::new(Recipe {
            descriptor: descriptor.clone(),
            operation: recipe,
            inputs: dependencies
                .iter()
                .map(|id| self.nodes[*id].key.clone())
                .collect(),
        }));
        if let Some(index) = self.keys.get(&key) {
            return Ok(*index);
        }
        let depth = dependencies
            .iter()
            .map(|id| self.nodes[*id].depth)
            .max()
            .unwrap_or(0)
            + 1;
        if self.nodes.len() >= self.limits.max_nodes
            || depth > self.limits.max_depth
            || payload
                > self
                    .limits
                    .max_payload_bytes
                    .saturating_sub(self.payload_bytes)
        {
            return self.checked(Err(error(
                ContentErrorKind::ResourceLimit,
                "graph node, depth, or payload budget exceeded",
            )));
        }
        let index = self.nodes.len();
        self.payload_bytes += payload;
        self.nodes.push(ContentNode {
            descriptor,
            operation,
            dependencies,
            key: key.clone(),
            depth,
        });
        self.keys.insert(key, index);
        Ok(index)
    }
    /// Describe a solid in its declared domain; negative zero is canonicalized.
    pub fn solid(&mut self, extent: ImageExtent, color: ColorValue) -> Result<ImageRef> {
        self.healthy()?;
        let format = if color.interpretation().color == ColorEncoding::Srgb {
            PixelFormat::Rgba8
        } else {
            PixelFormat::Rgba16Float
        };
        let descriptor = self.checked(ImageDescriptor::new(
            extent,
            format,
            color.interpretation(),
            1,
        ))?;
        let node = self.add(
            ResourceDescriptor::Image(descriptor),
            ContentOperation::Solid(color.clone()),
            RecipeOperation::Solid(color),
            vec![],
            0,
        )?;
        Ok(ImageRef {
            graph: self.id,
            node,
        })
    }
    /// Describe an immutable upload. Payload concatenates all tightly packed mips.
    /// Pixel values remain a source color/alpha contract, not a proven interpretation.
    pub fn upload_image(
        &mut self,
        descriptor: ImageDescriptor,
        bytes: Arc<[u8]>,
    ) -> Result<ImageRef> {
        self.healthy()?;
        if bytes.len() as u64 != descriptor.byte_len() {
            return self.checked(Err(error(
                ContentErrorKind::InvalidDescriptor,
                "image payload does not match mip storage size",
            )));
        }
        let len = bytes.len() as u64;
        let node = self.add(
            ResourceDescriptor::Image(descriptor),
            ContentOperation::UploadImage(bytes.clone()),
            RecipeOperation::UploadImage(bytes),
            vec![],
            len,
        )?;
        Ok(ImageRef {
            graph: self.id,
            node,
        })
    }
    /// Copy an entire image into a fresh logical version, without implicit conversion.
    pub fn copy_image(&mut self, source: ImageRef) -> Result<ImageRef> {
        let source = self.image_node(source)?;
        let descriptor = self.nodes[source].descriptor.clone();
        let node = self.add(
            descriptor,
            ContentOperation::CopyImage { source },
            RecipeOperation::CopyImage,
            vec![source],
            0,
        )?;
        Ok(ImageRef {
            graph: self.id,
            node,
        })
    }
    /// Retrieve a checked source descriptor without assuming destination-sized output.
    pub fn image_descriptor(&mut self, image: ImageRef) -> Result<&ImageDescriptor> {
        let node = self.image_node(image)?;
        match &self.nodes[node].descriptor {
            ResourceDescriptor::Image(d) => Ok(d),
            _ => unreachable!(),
        }
    }
    /// Describe a complete immutable buffer upload with exact layout length.
    pub fn upload_buffer(
        &mut self,
        descriptor: BufferDescriptor,
        bytes: Arc<[u8]>,
    ) -> Result<BufferRef> {
        self.healthy()?;
        if bytes.len() as u64 != descriptor.byte_len() {
            return self.checked(Err(error(
                ContentErrorKind::InvalidDescriptor,
                "buffer payload does not match descriptor",
            )));
        }
        let len = bytes.len() as u64;
        let node = self.add(
            ResourceDescriptor::Buffer(descriptor),
            ContentOperation::UploadBuffer(bytes.clone()),
            RecipeOperation::UploadBuffer(bytes),
            vec![],
            len,
        )?;
        Ok(BufferRef {
            graph: self.id,
            node,
        })
    }
    /// Describe initialized zero storage without constructing a CPU zero array.
    pub fn zeroed_buffer(&mut self, descriptor: BufferDescriptor) -> Result<BufferRef> {
        let node = self.add(
            ResourceDescriptor::Buffer(descriptor),
            ContentOperation::ZeroBuffer,
            RecipeOperation::ZeroBuffer,
            vec![],
            0,
        )?;
        Ok(BufferRef {
            graph: self.id,
            node,
        })
    }
    /// Validate slice bounds/alignment/declared usage. Errors poison the attempt.
    pub fn validate_buffer_slice(&mut self, slice: BufferSlice, usage: BufferUsage) -> Result<()> {
        let node = self.buffer_node(slice.buffer)?;
        let ResourceDescriptor::Buffer(d) = &self.nodes[node].descriptor else {
            unreachable!()
        };
        let valid = !usage.is_empty()
            && d.usage().contains(usage)
            && slice.byte_len > 0
            && slice
                .offset
                .is_multiple_of(u64::from(d.layout().alignment()))
            && slice.offset.is_multiple_of(u64::from(d.layout().stride()))
            && slice
                .byte_len
                .is_multiple_of(u64::from(d.layout().stride()))
            && slice
                .offset
                .checked_add(slice.byte_len)
                .is_some_and(|end| end <= d.byte_len());
        self.checked(if valid {
            Ok(())
        } else {
            Err(error(
                ContentErrorKind::InvalidDescriptor,
                "invalid buffer range, layout, or usage",
            ))
        })
    }
    /// Copy whole typed elements to a fresh buffer with compatible layout and size.
    pub fn copy_buffer(
        &mut self,
        source: BufferSlice,
        destination: BufferDescriptor,
    ) -> Result<BufferRef> {
        self.validate_buffer_slice(source, BufferUsage::COPY_SOURCE)?;
        let index = self.buffer_node(source.buffer)?;
        let ResourceDescriptor::Buffer(d) = &self.nodes[index].descriptor else {
            unreachable!()
        };
        if source.byte_len != destination.byte_len()
            || d.layout() != destination.layout()
            || !destination.usage().contains(BufferUsage::COPY_DESTINATION)
        {
            return self.checked(Err(error(
                ContentErrorKind::InvalidDescriptor,
                "copy destination size, layout, or usage mismatch",
            )));
        }
        let node = self.add(
            ResourceDescriptor::Buffer(destination),
            ContentOperation::CopyBuffer {
                source: index,
                offset: source.offset,
            },
            RecipeOperation::CopyBuffer {
                offset: source.offset,
            },
            vec![index],
            0,
        )?;
        Ok(BufferRef {
            graph: self.id,
            node,
        })
    }
}
