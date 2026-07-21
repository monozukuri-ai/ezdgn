use crate::numbers::decode_offset_binary_i32;
use crate::{DesignSettings, DgnError, MasterPoint, RawElementRef, RawPoint, V7Dimension};

const COMMON_HEADER_SIZE: usize = 36;

/// Raw element range decoded from offset-binary UOR values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementRange {
    pub dimension: V7Dimension,
    pub low: RawPoint,
    pub high: RawPoint,
}

impl ElementRange {
    /// Transforms this range to master units using the leading TCB settings.
    #[must_use]
    pub fn to_master(self, settings: DesignSettings) -> Option<MasterElementRange> {
        if self.dimension != settings.dimension {
            return None;
        }
        Some(MasterElementRange {
            low: settings.transform_point(self.low)?,
            high: settings.transform_point(self.high)?,
        })
    }
}

/// Element range after UOR scaling and global-origin translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MasterElementRange {
    pub low: MasterPoint,
    pub high: MasterPoint,
}

/// Decoded word-16 property flags. The `H` bit is intentionally left
/// contextual because it means hole, orphan cell, infinite line, or another
/// type-specific state depending on the element type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementProperties {
    pub raw: u16,
    pub class: u8,
    pub reserved: u8,
    pub locked: bool,
    pub new: bool,
    pub modified: bool,
    pub has_attributes: bool,
    pub screen_relative: bool,
    pub non_planar: bool,
    pub not_snappable: bool,
    pub h_bit: bool,
}

impl ElementProperties {
    fn decode(raw: u16) -> Self {
        Self {
            raw,
            class: (raw & 0x000f) as u8,
            reserved: ((raw >> 4) & 0x000f) as u8,
            locked: raw & 0x0100 != 0,
            new: raw & 0x0200 != 0,
            modified: raw & 0x0400 != 0,
            has_attributes: raw & 0x0800 != 0,
            screen_relative: raw & 0x1000 != 0,
            non_planar: raw & 0x2000 != 0,
            not_snappable: raw & 0x4000 != 0,
            h_bit: raw & 0x8000 != 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_raw_for_test(raw: u16) -> Self {
        Self::decode(raw)
    }

    #[must_use]
    pub const fn is_snappable(self) -> bool {
        !self.not_snappable
    }

    #[must_use]
    pub const fn is_planar(self) -> bool {
        !self.non_planar
    }
}

/// Color, line weight, and line style from word 17.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementSymbology {
    pub raw: u16,
    pub style: u8,
    pub weight: u8,
    pub color: u8,
}

impl ElementSymbology {
    fn decode(raw: u16) -> Self {
        Self {
            raw,
            style: (raw & 0x0007) as u8,
            weight: ((raw >> 3) & 0x001f) as u8,
            color: (raw >> 8) as u8,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_raw_for_test(raw: u16) -> Self {
        Self::decode(raw)
    }
}

/// Standard 18-word header shared by displayable V7 elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommonElementHeader {
    pub range: ElementRange,
    pub graphic_group: u16,
    pub attribute_index: u16,
    pub properties: ElementProperties,
    pub symbology: ElementSymbology,
    pub attribute_offset: Option<usize>,
    pub attribute_length: usize,
}

/// Whether this element type carries the standard display header.
#[must_use]
pub const fn element_type_has_common_header(element_type: u8) -> bool {
    !matches!(
        element_type,
        0 | 1 | 9 | 10 | 32 | 44 | 48 | 49 | 50 | 51 | 57 | 60 | 61 | 62 | 63
    )
}

/// Decodes the standard range/display header when the element type has one.
pub fn decode_common_header(
    record: RawElementRef<'_>,
    dimension: V7Dimension,
) -> Result<Option<CommonElementHeader>, DgnError> {
    if !element_type_has_common_header(record.header.element_type) {
        return Ok(None);
    }
    if record.bytes.len() < COMMON_HEADER_SIZE {
        return Err(DgnError::ElementTooShort {
            offset: record.offset,
            element_type: record.header.element_type,
            needed: COMMON_HEADER_SIZE,
            actual: record.bytes.len(),
            context: "common element header",
        });
    }

    let z = |offset| match dimension {
        V7Dimension::Two => None,
        V7Dimension::Three => Some(decode_offset_binary_i32(read_four(record.bytes, offset))),
    };
    let range = ElementRange {
        dimension,
        low: RawPoint {
            x: decode_offset_binary_i32(read_four(record.bytes, 4)),
            y: decode_offset_binary_i32(read_four(record.bytes, 8)),
            z: z(12),
        },
        high: RawPoint {
            x: decode_offset_binary_i32(read_four(record.bytes, 16)),
            y: decode_offset_binary_i32(read_four(record.bytes, 20)),
            z: z(24),
        },
    };
    let graphic_group = u16::from_le_bytes([record.bytes[28], record.bytes[29]]);
    let attribute_index = u16::from_le_bytes([record.bytes[30], record.bytes[31]]);
    let properties =
        ElementProperties::decode(u16::from_le_bytes([record.bytes[32], record.bytes[33]]));
    let symbology =
        ElementSymbology::decode(u16::from_le_bytes([record.bytes[34], record.bytes[35]]));

    let (attribute_offset, attribute_length) = if properties.has_attributes {
        let offset = 32 + usize::from(attribute_index) * 2;
        if !(COMMON_HEADER_SIZE..record.bytes.len()).contains(&offset) {
            return Err(DgnError::InvalidAttributeOffset {
                offset: record.offset,
                element_type: record.header.element_type,
                attribute_offset: offset,
                record_size: record.bytes.len(),
            });
        }
        (Some(offset), record.bytes.len() - offset)
    } else {
        (None, 0)
    };

    Ok(Some(CommonElementHeader {
        range,
        graphic_group,
        attribute_index,
        properties,
        symbology,
        attribute_offset,
        attribute_length,
    }))
}

fn read_four(bytes: &[u8], offset: usize) -> [u8; 4] {
    [
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RawElementHeader;

    #[test]
    fn decodes_every_property_and_symbology_bit() {
        let mut bytes = [0_u8; 40];
        bytes[..4].copy_from_slice(&[0x02, 0x03, 0x12, 0x00]);
        for (offset, value) in [(4, -2), (8, -1), (12, 0), (16, 1), (20, 2), (24, 3)] {
            bytes[offset..offset + 4].copy_from_slice(&encode_offset_binary(value));
        }
        bytes[30..32].copy_from_slice(&2_u16.to_le_bytes());
        bytes[32..34].copy_from_slice(&0xfff6_u16.to_le_bytes());
        bytes[34..36].copy_from_slice(&0xabad_u16.to_le_bytes());
        let record = RawElementRef {
            index: 0,
            offset: 128,
            header: RawElementHeader {
                level: 2,
                element_type: 3,
                complex_component: false,
                reserved: false,
                deleted: false,
                words_to_follow: 18,
            },
            bytes: &bytes,
        };

        let header = decode_common_header(record, V7Dimension::Three)
            .unwrap()
            .unwrap();
        assert_eq!(
            header.range.low,
            RawPoint {
                x: -2,
                y: -1,
                z: Some(0)
            }
        );
        assert_eq!(
            header.range.high,
            RawPoint {
                x: 1,
                y: 2,
                z: Some(3)
            }
        );
        assert_eq!(header.properties.class, 6);
        assert_eq!(header.properties.reserved, 15);
        assert!(header.properties.locked);
        assert!(header.properties.new);
        assert!(header.properties.modified);
        assert!(header.properties.has_attributes);
        assert!(header.properties.screen_relative);
        assert!(header.properties.non_planar);
        assert!(header.properties.not_snappable);
        assert!(header.properties.h_bit);
        assert!(!header.properties.is_planar());
        assert!(!header.properties.is_snappable());
        assert_eq!(header.symbology.style, 5);
        assert_eq!(header.symbology.weight, 21);
        assert_eq!(header.symbology.color, 171);
        assert_eq!(header.attribute_offset, Some(36));
        assert_eq!(header.attribute_length, 4);
    }

    #[test]
    fn rejects_short_display_header() {
        let bytes = [0x02, 0x03, 0x00, 0x00];
        let record = RawElementRef {
            index: 0,
            offset: 42,
            header: RawElementHeader {
                level: 2,
                element_type: 3,
                complex_component: false,
                reserved: false,
                deleted: false,
                words_to_follow: 0,
            },
            bytes: &bytes,
        };
        assert!(matches!(
            decode_common_header(record, V7Dimension::Two),
            Err(DgnError::ElementTooShort {
                offset: 42,
                element_type: 3,
                needed: 36,
                actual: 4,
                context: "common element header",
            })
        ));
    }

    fn encode_offset_binary(value: i32) -> [u8; 4] {
        let encoded = (value as u32) ^ 0x8000_0000;
        [
            (encoded >> 16) as u8,
            (encoded >> 24) as u8,
            encoded as u8,
            (encoded >> 8) as u8,
        ]
    }
}
