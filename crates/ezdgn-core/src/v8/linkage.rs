use std::sync::Arc;

use crate::DgnError;

use super::raw::{read_u32, V8ScanOptions};

/// Property-linkage family used for model strings and named properties.
pub const V8_PROPERTY_LINKAGE_KIND: u32 = 0x0056_d210;

/// One independently framed inline V8 attribute linkage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V8Linkage {
    pub offset: usize,
    pub words_to_follow: Option<u8>,
    pub kind_code: Option<u32>,
    pub property_id: Option<u32>,
    pub property_bytes: Option<Arc<[u8]>>,
    pub property_text: Option<String>,
    pub complete: bool,
    pub raw_bytes: Arc<[u8]>,
}

/// Split an inline attribute region into its native word-count-framed records.
///
/// Unknown kinds remain exact raw records. A malformed final record is retained
/// as one incomplete linkage instead of being silently discarded.
pub fn decode_v8_linkages(
    bytes: &[u8],
    options: V8ScanOptions,
    context: &str,
) -> Result<Vec<V8Linkage>, DgnError> {
    let mut linkages = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let remaining = &bytes[offset..];
        if remaining.len() < 4 {
            linkages.push(incomplete_linkage(offset, remaining));
            break;
        }
        let signature = read_u32(remaining, 0).unwrap_or_default();
        let words_to_follow = signature as u8;
        let size = usize::from(words_to_follow)
            .checked_add(1)
            .and_then(|words| words.checked_mul(2))
            .unwrap_or(usize::MAX);
        if size < 4 || size > remaining.len() {
            linkages.push(incomplete_linkage(offset, remaining));
            break;
        }
        let raw = &remaining[..size];
        let kind_code = signature >> 8;
        let mut property_id = None;
        let mut property_bytes = None;
        let mut property_text = None;
        let mut complete = true;
        if kind_code == V8_PROPERTY_LINKAGE_KIND {
            if raw.len() < 12 {
                complete = false;
            } else {
                property_id = read_u32(raw, 4);
                let declared =
                    usize::try_from(read_u32(raw, 8).unwrap_or_default()).unwrap_or(usize::MAX);
                if declared > options.max_string_bytes {
                    return Err(DgnError::V8StringLimitExceeded {
                        context: format!("{context} property linkage"),
                        actual: declared,
                        limit: options.max_string_bytes,
                    });
                }
                if let Some(payload) = raw.get(12..12usize.saturating_add(declared)) {
                    property_bytes = Some(Arc::from(payload));
                    property_text = decode_property_text(payload);
                } else {
                    complete = false;
                }
            }
        }
        linkages.push(V8Linkage {
            offset,
            words_to_follow: Some(words_to_follow),
            kind_code: Some(kind_code),
            property_id,
            property_bytes,
            property_text,
            complete,
            raw_bytes: Arc::from(raw),
        });
        offset += size;
    }
    Ok(linkages)
}

fn incomplete_linkage(offset: usize, raw: &[u8]) -> V8Linkage {
    V8Linkage {
        offset,
        words_to_follow: raw.first().copied(),
        kind_code: (raw.len() >= 4).then(|| read_u32(raw, 0).unwrap_or_default() >> 8),
        property_id: None,
        property_bytes: None,
        property_text: None,
        complete: false,
        raw_bytes: Arc::from(raw),
    }
}

pub(crate) fn decode_property_text(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.strip_suffix(&[0, 0]).unwrap_or(bytes);
    if let Some(utf16) = bytes.strip_prefix(&[0xff, 0xfd]) {
        if utf16.len() % 2 != 0 {
            return None;
        }
        let units = utf16
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16(&units).ok();
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .ok()
        .or_else(|| Some(decode_windows_1252(bytes)))
}

pub(crate) fn decode_windows_1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match *byte {
            0x80 => '\u{20ac}',
            0x82 => '\u{201a}',
            0x83 => '\u{0192}',
            0x84 => '\u{201e}',
            0x85 => '\u{2026}',
            0x86 => '\u{2020}',
            0x87 => '\u{2021}',
            0x88 => '\u{02c6}',
            0x89 => '\u{2030}',
            0x8a => '\u{0160}',
            0x8b => '\u{2039}',
            0x8c => '\u{0152}',
            0x8e => '\u{017d}',
            0x91 => '\u{2018}',
            0x92 => '\u{2019}',
            0x93 => '\u{201c}',
            0x94 => '\u{201d}',
            0x95 => '\u{2022}',
            0x96 => '\u{2013}',
            0x97 => '\u{2014}',
            0x98 => '\u{02dc}',
            0x99 => '\u{2122}',
            0x9a => '\u{0161}',
            0x9b => '\u{203a}',
            0x9c => '\u{0153}',
            0x9e => '\u{017e}',
            0x9f => '\u{0178}',
            value => char::from(value),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_property_linkage_and_preserves_unknown_or_trailing_data() {
        let mut property = vec![0x0f, 0x10, 0xd2, 0x56];
        property.extend(1_u32.to_le_bytes());
        property.extend(18_u32.to_le_bytes());
        property.extend([0xff, 0xfd]);
        for unit in "my_model".encode_utf16() {
            property.extend(unit.to_le_bytes());
        }
        property.extend([0, 0]);
        let decoded = decode_v8_linkages(&property, V8ScanOptions::default(), "test").unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].property_id, Some(1));
        assert_eq!(decoded[0].property_text.as_deref(), Some("my_model"));
        assert!(decoded[0].complete);

        let malformed = decode_v8_linkages(&[9, 1, 2], V8ScanOptions::default(), "test").unwrap();
        assert_eq!(malformed.len(), 1);
        assert!(!malformed[0].complete);
        assert_eq!(malformed[0].raw_bytes.as_ref(), [9, 1, 2]);
    }
}
