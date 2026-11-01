use super::{ContentError, ContentErrorKind};
use std::{collections::BTreeMap, sync::Arc};

/// A scalar/vector field reflected from a portable shader interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderValueType {
    /// One through four 32-bit floats.
    Float(u8),
    /// One through four signed 32-bit integers.
    Sint(u8),
    /// One through four unsigned 32-bit integers.
    Uint(u8),
}
/// Reflected host-shareable uniform member type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UniformType {
    /// Scalar or vector with its exact shader scalar type.
    Value(ShaderValueType),
    /// Column-major f32 matrix, including reflected padding between columns.
    Matrix {
        /// Number of columns.
        columns: u8,
        /// Number of rows per column.
        rows: u8,
        /// Byte distance between adjacent columns.
        column_stride: u32,
    },
}
/// Named uniform field; offsets come from validated WGSL, never caller ABI claims.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UniformField {
    pub(super) name: Arc<str>,
    pub(super) offset: u32,
    pub(super) ty: UniformType,
}
impl UniformField {
    /// WGSL struct member name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Offset in the reflected uniform allocation.
    pub fn offset(&self) -> u32 {
        self.offset
    }
    /// Exact scalar/vector/matrix member type.
    pub fn ty(&self) -> UniformType {
        self.ty
    }
}
/// Complete reflected uniform struct. Nested structs/arrays are not yet supported.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UniformLayout {
    pub(super) byte_len: u32,
    pub(super) fields: Vec<UniformField>,
}
impl UniformLayout {
    /// Defined allocation bytes, including zero-filled padding.
    pub fn byte_len(&self) -> u32 {
        self.byte_len
    }
    /// Reflected members in declaration order.
    pub fn fields(&self) -> &[UniformField] {
        &self.fields
    }
    /// Validate names, types, arity and finiteness, and pack reflected offsets.
    /// All members must be supplied; unused padding is initialized to zero.
    pub fn pack(&self, values: &UniformValues) -> Result<Arc<[u8]>, ContentError> {
        let invalid = |message| ContentError::new(ContentErrorKind::InvalidDescriptor, message);
        if values.0.len() != self.fields.len()
            || values
                .0
                .keys()
                .any(|name| !self.fields.iter().any(|f| f.name.as_ref() == name))
        {
            return Err(invalid(
                "uniform names must match all reflected struct members",
            ));
        }
        let mut bytes = vec![0; self.byte_len as usize];
        for field in &self.fields {
            let value = values
                .0
                .get(field.name.as_ref())
                .ok_or_else(|| invalid("missing uniform member"))?;
            let (words, columns, rows, stride) = match (field.ty, value) {
                (UniformType::Value(ShaderValueType::Float(lanes)), UniformValue::Float(v))
                    if v.len() == lanes as usize =>
                {
                    (float_words(v)?, 1, lanes as usize, 0)
                }
                (UniformType::Value(ShaderValueType::Sint(lanes)), UniformValue::Sint(v))
                    if v.len() == lanes as usize =>
                {
                    (v.iter().map(|v| *v as u32).collect(), 1, lanes as usize, 0)
                }
                (UniformType::Value(ShaderValueType::Uint(lanes)), UniformValue::Uint(v))
                    if v.len() == lanes as usize =>
                {
                    (v.clone(), 1, lanes as usize, 0)
                }
                (
                    UniformType::Matrix {
                        columns,
                        rows,
                        column_stride,
                    },
                    UniformValue::Matrix {
                        columns: c,
                        rows: r,
                        values,
                    },
                ) if columns == *c
                    && rows == *r
                    && values.len() == columns as usize * rows as usize =>
                {
                    (
                        float_words(values)?,
                        columns as usize,
                        rows as usize,
                        column_stride as usize,
                    )
                }
                _ => {
                    return Err(invalid(
                        "uniform member scalar type, shape, or arity mismatch",
                    ));
                }
            };
            for column in 0..columns {
                for row in 0..rows {
                    let offset = field.offset as usize + column * stride + row * 4;
                    bytes[offset..offset + 4]
                        .copy_from_slice(&words[column * rows + row].to_le_bytes());
                }
            }
        }
        Ok(bytes.into())
    }
}
fn float_words(values: &[f32]) -> Result<Vec<u32>, ContentError> {
    if values.iter().any(|v| !v.is_finite()) {
        return Err(ContentError::new(
            ContentErrorKind::InvalidDescriptor,
            "uniform floats must be finite",
        ));
    }
    Ok(values
        .iter()
        .map(|v| if *v == 0.0 { 0 } else { v.to_bits() })
        .collect())
}
#[derive(Clone, Debug)]
enum UniformValue {
    Float(Vec<f32>),
    Sint(Vec<i32>),
    Uint(Vec<u32>),
    Matrix {
        columns: u8,
        rows: u8,
        values: Vec<f32>,
    },
}
/// Typed immutable-snapshot inputs. Setters replace a previously supplied name.
/// Validation occurs when packing against a reflected `UniformLayout`.
#[derive(Clone, Debug, Default)]
pub struct UniformValues(BTreeMap<String, UniformValue>);
impl UniformValues {
    /// No named values yet.
    pub fn new() -> Self {
        Self::default()
    }
    /// Set a scalar f32 member.
    pub fn float(self, name: impl Into<String>, value: f32) -> Self {
        self.floats(name, vec![value])
    }
    /// Set a vec2<f32> member.
    pub fn vec2(self, name: impl Into<String>, value: [f32; 2]) -> Self {
        self.floats(name, value.to_vec())
    }
    /// Set a vec3<f32> member.
    pub fn vec3(self, name: impl Into<String>, value: [f32; 3]) -> Self {
        self.floats(name, value.to_vec())
    }
    /// Set a vec4<f32> member.
    pub fn vec4(self, name: impl Into<String>, value: [f32; 4]) -> Self {
        self.floats(name, value.to_vec())
    }
    fn floats(mut self, name: impl Into<String>, values: Vec<f32>) -> Self {
        self.0.insert(name.into(), UniformValue::Float(values));
        self
    }
    /// Set a scalar or vector of signed integers; arity must match reflection.
    pub fn ints(mut self, name: impl Into<String>, values: impl Into<Vec<i32>>) -> Self {
        self.0
            .insert(name.into(), UniformValue::Sint(values.into()));
        self
    }
    /// Set a scalar or vector of unsigned integers; arity must match reflection.
    pub fn uints(mut self, name: impl Into<String>, values: impl Into<Vec<u32>>) -> Self {
        self.0
            .insert(name.into(), UniformValue::Uint(values.into()));
        self
    }
    /// Set column-major f32 matrix values without ABI padding in the input.
    pub fn matrix(
        mut self,
        name: impl Into<String>,
        columns: u8,
        rows: u8,
        values: impl Into<Vec<f32>>,
    ) -> Self {
        self.0.insert(
            name.into(),
            UniformValue::Matrix {
                columns,
                rows,
                values: values.into(),
            },
        );
        self
    }
}
