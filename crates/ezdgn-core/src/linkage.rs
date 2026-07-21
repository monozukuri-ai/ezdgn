//! Lossless decoding of V7 attribute linkage areas.

use crate::{CommonElementHeader, RawElementRef};

/// One signed sub-UOR correction pair from a high-precision linkage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrecisionDelta {
    pub x: i16,
    pub y: i16,
}

/// Typed interpretation of a linkage while its complete raw bytes remain
/// available on [`AttributeLinkage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkageData {
    /// Eight-byte Design Manager/Relational Interface linkage.
    Dmrs { entity_number: u16, mslink: u32 },
    /// Sixteen-byte external database linkage.
    Database { entity_number: u16, mslink: u32 },
    /// MicroStation shape-fill user linkage (`0x0041`).
    ShapeFill { color_index: u8 },
    /// Element association identifier linkage (`0x7d2f`).
    AssociationId { association_id: u32 },
    /// Sub-UOR coordinate corrections (`0x51a9`).
    HighPrecision {
        delta_words: u16,
        deltas: Vec<PrecisionDelta>,
        /// False when the declared delta area does not fit in this linkage.
        complete: bool,
    },
    /// A structurally valid user linkage whose payload is not interpreted.
    User,
    /// Remaining attribute bytes that do not form a bounded linkage.
    Unparsed,
}

impl LinkageData {
    /// Stable public name used by the Python object model and CLI.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Dmrs { .. } => "DMRS",
            Self::Database { .. } => "DATABASE",
            Self::ShapeFill { .. } => "SHAPE_FILL",
            Self::AssociationId { .. } => "ASSOCIATION_ID",
            Self::HighPrecision { .. } => "HIGH_PRECISION",
            Self::User => "USER",
            Self::Unparsed => "UNPARSED",
        }
    }
}

/// One linkage in an element's attribute area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeLinkage<'a> {
    /// Byte offset from the beginning of the containing element record.
    pub offset: usize,
    /// Linkage type/user ID, or `None` for an unparseable tail.
    pub linkage_type: Option<u16>,
    /// Size derived from the linkage header, or `None` for an unparseable tail.
    pub declared_size: Option<usize>,
    /// Complete bounded bytes for this linkage or unparseable tail.
    pub raw: &'a [u8],
    pub data: LinkageData,
}

/// Decodes every bounded linkage and retains an unparseable trailing region as
/// one final item. Unknown user IDs never make the enclosing element fail.
#[must_use]
pub fn decode_attribute_linkages<'a>(
    record: RawElementRef<'a>,
    header: Option<CommonElementHeader>,
) -> Vec<AttributeLinkage<'a>> {
    let Some(attribute_offset) = header.and_then(|header| header.attribute_offset) else {
        return Vec::new();
    };
    let attributes = &record.bytes[attribute_offset..];
    let mut result = Vec::new();
    let mut cursor = 0_usize;

    while cursor < attributes.len() {
        let remaining = &attributes[cursor..];
        let Some((declared_size, linkage_type, is_dmrs)) = linkage_header(remaining) else {
            result.push(AttributeLinkage {
                offset: attribute_offset + cursor,
                linkage_type: None,
                declared_size: None,
                raw: remaining,
                data: LinkageData::Unparsed,
            });
            break;
        };
        if declared_size > remaining.len() {
            result.push(AttributeLinkage {
                offset: attribute_offset + cursor,
                linkage_type: Some(linkage_type),
                declared_size: Some(declared_size),
                raw: remaining,
                data: LinkageData::Unparsed,
            });
            break;
        }

        let raw = &remaining[..declared_size];
        result.push(AttributeLinkage {
            offset: attribute_offset + cursor,
            linkage_type: Some(linkage_type),
            declared_size: Some(declared_size),
            raw,
            data: decode_linkage_data(raw, linkage_type, is_dmrs),
        });
        cursor += declared_size;
    }

    result
}

fn linkage_header(bytes: &[u8]) -> Option<(usize, u16, bool)> {
    if bytes.len() < 4 {
        return None;
    }
    if bytes[0] == 0 && matches!(bytes[1], 0 | 0x80) {
        return Some((8, 0, true));
    }
    if bytes[1] & 0x10 == 0 {
        return None;
    }
    let size = usize::from(bytes[0]) * 2 + 2;
    if size < 4 {
        return None;
    }
    Some((size, u16::from_le_bytes([bytes[2], bytes[3]]), false))
}

fn decode_linkage_data(raw: &[u8], linkage_type: u16, is_dmrs: bool) -> LinkageData {
    if is_dmrs && raw.len() >= 8 {
        return LinkageData::Dmrs {
            entity_number: u16::from_le_bytes([raw[2], raw[3]]),
            mslink: u32::from(raw[4]) | (u32::from(raw[5]) << 8) | (u32::from(raw[6]) << 16),
        };
    }
    if linkage_type == 0x0041 && raw.len() >= 9 {
        return LinkageData::ShapeFill {
            color_index: raw[8],
        };
    }
    if linkage_type == 0x7d2f && raw.len() >= 8 {
        return LinkageData::AssociationId {
            association_id: u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
        };
    }
    if linkage_type == 0x51a9 && raw.len() >= 8 {
        let delta_words = u16::from_le_bytes([raw[4], raw[5]]);
        let delta_bytes = usize::from(delta_words) * 2;
        let complete = delta_bytes <= raw.len() - 8 && delta_bytes % 4 == 0;
        let deltas = if complete {
            raw[8..8 + delta_bytes]
                .chunks_exact(4)
                .map(|chunk| PrecisionDelta {
                    x: i16::from_le_bytes([chunk[0], chunk[1]]),
                    y: i16::from_le_bytes([chunk[2], chunk[3]]),
                })
                .collect()
        } else {
            Vec::new()
        };
        return LinkageData::HighPrecision {
            delta_words,
            deltas,
            complete,
        };
    }
    if raw.len() == 16 {
        return LinkageData::Database {
            entity_number: u16::from_le_bytes([raw[6], raw[7]]),
            mslink: u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]),
        };
    }
    LinkageData::User
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ElementProperties, ElementRange, ElementSymbology, RawElementHeader, RawPoint, V7Dimension,
    };

    fn record<'a>(
        bytes: &'a [u8],
        attribute_offset: usize,
    ) -> (RawElementRef<'a>, CommonElementHeader) {
        (
            RawElementRef {
                index: 0,
                offset: 100,
                header: RawElementHeader {
                    level: 1,
                    element_type: 3,
                    complex_component: false,
                    reserved: false,
                    deleted: false,
                    words_to_follow: (bytes.len() / 2 - 2) as u16,
                },
                bytes,
            },
            CommonElementHeader {
                range: ElementRange {
                    dimension: V7Dimension::Two,
                    low: RawPoint {
                        x: 0,
                        y: 0,
                        z: None,
                    },
                    high: RawPoint {
                        x: 0,
                        y: 0,
                        z: None,
                    },
                },
                graphic_group: 0,
                attribute_index: ((attribute_offset - 32) / 2) as u16,
                properties: ElementProperties::from_raw_for_test(0x0800),
                symbology: ElementSymbology::from_raw_for_test(0),
                attribute_offset: Some(attribute_offset),
                attribute_length: bytes.len() - attribute_offset,
            },
        )
    }

    #[test]
    fn decodes_known_and_unknown_linkages_without_losing_bytes() {
        let mut bytes = vec![0_u8; 36];
        bytes.extend_from_slice(&[3, 0x10, 0x2f, 0x7d, 0x78, 0x56, 0x34, 0x12]);
        bytes.extend_from_slice(&[
            7, 0x10, 0xa9, 0x51, 4, 0, 0, 0, 0xff, 0xff, 2, 0, 3, 0, 0xfc, 0xff,
        ]);
        bytes.extend_from_slice(&[3, 0x10, 0x34, 0x12, 1, 2, 3, 4]);
        let (record, header) = record(&bytes, 36);
        let links = decode_attribute_linkages(record, Some(header));
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].offset, 36);
        assert!(matches!(
            links[0].data,
            LinkageData::AssociationId {
                association_id: 0x1234_5678
            }
        ));
        assert!(matches!(
            &links[1].data,
            LinkageData::HighPrecision {
                delta_words: 4,
                deltas,
                complete: true,
            } if deltas == &[PrecisionDelta { x: -1, y: 2 }, PrecisionDelta { x: 3, y: -4 }]
        ));
        assert_eq!(links[2].linkage_type, Some(0x1234));
        assert!(matches!(links[2].data, LinkageData::User));
        assert_eq!(links.iter().map(|link| link.raw.len()).sum::<usize>(), 32);
    }

    #[test]
    fn retains_invalid_trailing_linkage_as_unparsed() {
        let mut bytes = vec![0_u8; 36];
        bytes.extend_from_slice(&[7, 0x10, 0xa9, 0x51, 10, 0, 0, 0]);
        let (record, header) = record(&bytes, 36);
        let links = decode_attribute_linkages(record, Some(header));
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].declared_size, Some(16));
        assert_eq!(links[0].raw.len(), 8);
        assert!(matches!(links[0].data, LinkageData::Unparsed));
    }
}
