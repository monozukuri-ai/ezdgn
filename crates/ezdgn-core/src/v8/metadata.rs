use std::sync::Arc;

use crate::DgnError;

use super::linkage::{decode_v8_linkages, V8Linkage};
use super::raw::{read_u32, read_u64, V8RawModel, V8ScanOptions};

const MODEL_HEADER_TYPE: u16 = 66;
const MODEL_DIMENSION_2D_FLAG: u32 = 0x0080_0000;
const MODEL_ID_OFFSET: usize = 0x18;
const MODEL_EXTENTS_OFFSET: usize = 0x90;
const MODEL_ORIGIN_OFFSET: usize = 0xc8;
const MODEL_UOR_PER_MASTER_OFFSET: usize = 0xe0;

/// Native dimensionality declared by a V8 model header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V8Dimension {
    Two,
    Three,
}

impl V8Dimension {
    #[must_use]
    pub const fn value(self) -> u8 {
        match self {
            Self::Two => 2,
            Self::Three => 3,
        }
    }
}

/// Three-coordinate point used by V8 model and element metadata.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct V8Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V8Point3 {
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    #[must_use]
    pub const fn as_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
}

/// Integer UOR range retained from a model or common element header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V8Range3I64 {
    pub low: [i64; 3],
    pub high: [i64; 3],
}

/// Master-unit range derived from a validated model scale and origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct V8Range3 {
    pub low: V8Point3,
    pub high: V8Point3,
}

/// Parsed metadata from one model-index entry and its type-66 header object.
#[derive(Debug, Clone)]
pub struct V8ModelMetadata {
    pub index: usize,
    pub storage_path: String,
    pub storage_index: u16,
    pub model_number: u16,
    pub model_id: u64,
    pub index_model_id: u64,
    pub name: String,
    pub description: String,
    pub dimension: V8Dimension,
    pub type_and_flags: u32,
    pub model_flags: u32,
    pub uor_per_master: f64,
    pub scale: f64,
    pub global_origin_uor: V8Point3,
    pub extents_uor: V8Range3I64,
    pub extents_master: V8Range3,
    pub master_unit: Option<String>,
    pub sub_unit: Option<String>,
    pub linkages: Vec<V8Linkage>,
    pub raw_header: Arc<[u8]>,
}

impl V8ModelMetadata {
    /// Convert stored UOR coordinates into the model's master-unit coordinates.
    #[must_use]
    pub fn to_master(&self, point: V8Point3) -> V8Point3 {
        V8Point3::new(
            (point.x - self.global_origin_uor.x) * self.scale,
            (point.y - self.global_origin_uor.y) * self.scale,
            (point.z - self.global_origin_uor.z) * self.scale,
        )
    }
}

pub(crate) fn decode_model_metadata(
    model: &V8RawModel,
    options: V8ScanOptions,
) -> Result<V8ModelMetadata, DgnError> {
    let path = &model.model_header_stream.path;
    let record = find_model_header(model)?;
    if record.len() < MODEL_UOR_PER_MASTER_OFFSET + 8 {
        return Err(DgnError::InvalidV8ModelHeader {
            path: path.clone(),
            context: format!(
                "type-66 object is {} bytes; metadata fields require {}",
                record.len(),
                MODEL_UOR_PER_MASTER_OFFSET + 8
            ),
        });
    }
    let type_and_flags = read_u32(record, 0).unwrap_or_default();
    let words = read_u32(record, 4).unwrap_or_default();
    let attribute_words = read_u32(record, 8).unwrap_or_default();
    let attribute_offset = usize::try_from(attribute_words)
        .unwrap_or(usize::MAX)
        .checked_mul(2)
        .ok_or_else(|| DgnError::InvalidV8ModelHeader {
            path: path.clone(),
            context: "attribute offset overflows".to_owned(),
        })?;
    if attribute_words > words || attribute_offset > record.len() {
        return Err(DgnError::InvalidV8ModelHeader {
            path: path.clone(),
            context: format!(
                "attribute boundary {attribute_words} words exceeds object size {words} words"
            ),
        });
    }
    let uor_per_master = read_f64(record, MODEL_UOR_PER_MASTER_OFFSET).ok_or_else(|| {
        DgnError::InvalidV8ModelHeader {
            path: path.clone(),
            context: "UOR-per-master field is truncated".to_owned(),
        }
    })?;
    if !uor_per_master.is_finite() || uor_per_master <= 0.0 {
        return Err(DgnError::InvalidV8ModelHeader {
            path: path.clone(),
            context: format!("invalid UOR-per-master value {uor_per_master}"),
        });
    }
    let scale = uor_per_master.recip();
    let origin = V8Point3::new(
        read_f64(record, MODEL_ORIGIN_OFFSET).unwrap_or_default(),
        read_f64(record, MODEL_ORIGIN_OFFSET + 8).unwrap_or_default(),
        read_f64(record, MODEL_ORIGIN_OFFSET + 16).unwrap_or_default(),
    );
    if !origin.as_array().iter().all(|value| value.is_finite()) {
        return Err(DgnError::InvalidV8ModelHeader {
            path: path.clone(),
            context: "global origin contains a non-finite value".to_owned(),
        });
    }
    let extents_uor = V8Range3I64 {
        low: [
            read_i64(record, MODEL_EXTENTS_OFFSET).unwrap_or_default(),
            read_i64(record, MODEL_EXTENTS_OFFSET + 8).unwrap_or_default(),
            read_i64(record, MODEL_EXTENTS_OFFSET + 16).unwrap_or_default(),
        ],
        high: [
            read_i64(record, MODEL_EXTENTS_OFFSET + 24).unwrap_or_default(),
            read_i64(record, MODEL_EXTENTS_OFFSET + 32).unwrap_or_default(),
            read_i64(record, MODEL_EXTENTS_OFFSET + 40).unwrap_or_default(),
        ],
    };
    let extents_master = V8Range3 {
        low: model_point_to_master(extents_uor.low, origin, scale),
        high: model_point_to_master(extents_uor.high, origin, scale),
    };
    let linkages = decode_v8_linkages(
        &record[attribute_offset..],
        options,
        &format!("model {}", model.index.index),
    )?;
    let property = |id| {
        linkages
            .iter()
            .find(|linkage| linkage.property_id == Some(id))
            .and_then(|linkage| linkage.property_text.clone())
    };
    // The storage path is the authoritative association between a model-index
    // entry and its header.  Files produced by older V8 applications have been
    // observed with different non-zero values in the index and header fields,
    // so retain both instead of rejecting an otherwise unambiguous model.
    let header_model_id = read_u64(record, MODEL_ID_OFFSET).unwrap_or_default();

    Ok(V8ModelMetadata {
        index: model.index.index,
        storage_path: model.storage_path.clone(),
        storage_index: model.index.storage_index,
        model_number: model.index.model_number,
        model_id: header_model_id,
        index_model_id: model.index.model_id,
        name: model.index.name.clone(),
        description: model.index.description.clone(),
        dimension: if type_and_flags & MODEL_DIMENSION_2D_FLAG != 0 {
            V8Dimension::Two
        } else {
            V8Dimension::Three
        },
        type_and_flags,
        model_flags: read_u32(record, 0x10).unwrap_or_default(),
        uor_per_master,
        scale,
        global_origin_uor: origin,
        extents_uor,
        extents_master,
        master_unit: property(19),
        sub_unit: property(20),
        linkages,
        raw_header: Arc::from(record),
    })
}

fn find_model_header(model: &V8RawModel) -> Result<&[u8], DgnError> {
    let bytes = model.model_header_bytes.as_ref();
    let mut candidates = Vec::new();
    for delimiter_offset in (0..bytes.len().saturating_sub(16)).step_by(4) {
        if read_u32(bytes, delimiter_offset) != Some(0) {
            continue;
        }
        let record_offset = delimiter_offset + 4;
        let Some(type_and_flags) = read_u32(bytes, record_offset) else {
            continue;
        };
        if type_and_flags as u16 != MODEL_HEADER_TYPE {
            continue;
        }
        let Some(words) = read_u32(bytes, record_offset + 4) else {
            continue;
        };
        let Some(size) = usize::try_from(words)
            .ok()
            .and_then(|value| value.checked_mul(2))
        else {
            continue;
        };
        let Some(end) = record_offset.checked_add(size) else {
            continue;
        };
        if end != bytes.len() || size < 32 {
            continue;
        }
        candidates.push(&bytes[record_offset..end]);
    }
    if candidates.len() == 1 {
        return Ok(candidates[0]);
    }
    let matching: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|record| {
            model.index.model_id != 0
                && read_u64(record, MODEL_ID_OFFSET) == Some(model.index.model_id)
        })
        .collect();
    match matching.as_slice() {
        [record] => Ok(*record),
        _ if candidates.is_empty() => Err(DgnError::InvalidV8ModelHeader {
            path: model.model_header_stream.path.clone(),
            context: "no complete type-66 object was found".to_owned(),
        }),
        _ => Err(DgnError::InvalidV8ModelHeader {
            path: model.model_header_stream.path.clone(),
            context: format!(
                "{} complete type-66 objects were found and no unique index-id match exists",
                candidates.len()
            ),
        }),
    }
}

fn model_point_to_master(raw: [i64; 3], origin: V8Point3, scale: f64) -> V8Point3 {
    V8Point3::new(
        (raw[0] as f64 - origin.x) * scale,
        (raw[1] as f64 - origin.y) * scale,
        (raw[2] as f64 - origin.z) * scale,
    )
}

pub(crate) fn read_i64(bytes: &[u8], offset: usize) -> Option<i64> {
    read_u64(bytes, offset).map(|value| i64::from_le_bytes(value.to_le_bytes()))
}

pub(crate) fn read_f64(bytes: &[u8], offset: usize) -> Option<f64> {
    read_u64(bytes, offset).map(f64::from_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_conversion_applies_scale_and_origin() {
        assert_eq!(
            model_point_to_master([12, 24, 36], V8Point3::new(2.0, 4.0, 6.0), 0.1),
            V8Point3::new(1.0, 2.0, 3.0)
        );
    }
}
