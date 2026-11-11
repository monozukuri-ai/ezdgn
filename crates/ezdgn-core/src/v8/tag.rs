use std::sync::Arc;

use super::linkage::{decode_v8_linkages, decode_windows_1252, V8Linkage};
use super::metadata::read_f64;
use super::raw::{read_u16, read_u32, read_u64, V8RawDocument, V8RawObject, V8ScanOptions};

/// Value type of a tag whose value is 8-bit text.
pub const V8_TAG_VALUE_TEXT: u16 = 1;
/// Value type of a tag whose value is a 32-bit integer.
pub const V8_TAG_VALUE_INTEGER: u16 = 3;
/// The eight bytes at `0x28` of a named object that holds a tag set.
pub const V8_TAG_SET_CODE: [u8; 8] = [b't', b'e', b'S', b't', 0x81, 0x0f, 0x00, 0x00];
/// Most definition bytes that one tag set may hold, and most definition
/// bytes that one call decodes for all tag sets of a document. A definition
/// needs at least 12 bytes, so this also bounds the number of definitions
/// and the memory of the decoded result.
pub const V8_MAX_TAG_SET_BYTES: usize = 1024 * 1024;

const TAG_SET_ELEMENT_TYPE: u16 = 39;
const TAG_SET_CODE_OFFSET: usize = 0x28;
const TAG_SET_SIZE_OFFSET: usize = 0x34;
const TAG_SET_DEFINITIONS_OFFSET: usize = 0x3c;
/// Two 16-bit words after the value type of a definition. The two observed
/// writers store different things there, so they are skipped.
const TAG_SET_UNRESOLVED_BYTES: usize = 4;

const TAG_ANCHOR_OFFSET: usize = 0xa0;
const TAG_OFFSET_OFFSET: usize = 0xb8;
const TAG_ID_OFFSET: usize = 0xd0;
const TAG_VALUE_TYPE_OFFSET: usize = 0xd2;
const TAG_VALUE_SIZE_OFFSET: usize = 0x138;
const TAG_CODE_PAGE_OFFSET: usize = 0x13c;
const TAG_VALUE_OFFSET: usize = 0x140;
const WINDOWS_1252: u32 = 1252;

/// Kind code of the linkages that refer to other elements by their ID.
const REFERENCE_LINKAGE_KIND: u32 = 0x0056_d010;
const SET_LINKAGE_BYTES: usize = 24;
const SET_LINKAGE_HEAD: [u8; 8] = [0x17, 0x27, 0x00, 0x00, 0x00, 0x02, 0x01, 0x00];
const SET_LINKAGE_ID_OFFSET: usize = 12;
const HOST_LINKAGE_BYTES: usize = 56;
const HOST_LINKAGE_HEAD: [u8; 4] = [0x10, 0x27, 0x01, 0x00];
const HOST_LINKAGE_ROOT_OFFSET: usize = 10;
const HOST_LINKAGE_ROOT: [u8; 10] = [0x01, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
const HOST_LINKAGE_ID_OFFSET: usize = 20;

/// A decoded tag value or tag default.
///
/// Other value types exist and are not decoded yet (type 5 was seen), so a
/// match on this enum needs a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum V8TagValue {
    Text(String),
    Integer(i32),
}

/// One tag definition of a tag set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V8TagDefinition {
    /// Name of the tag.
    pub name: String,
    /// Number that a tag element stores to name this definition.
    pub tag_id: u16,
    /// Text shown to a person who enters the value; may be empty.
    pub prompt: String,
    /// [`V8_TAG_VALUE_TEXT`] or [`V8_TAG_VALUE_INTEGER`].
    pub value_type: u16,
    /// Default value, of the type that `value_type` names.
    pub default: V8TagValue,
}

/// One tag set: a named object that holds the definitions of its tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V8TagSet {
    /// Element ID of the named object. A tag element refers to it.
    pub element_id: u64,
    /// Name from the name linkage; empty when the object has none.
    pub name: String,
    /// The definitions in stored order.
    pub definitions: Vec<V8TagDefinition>,
}

impl V8TagSet {
    /// The first definition with this tag ID.
    #[must_use]
    pub fn definition(&self, tag_id: u16) -> Option<&V8TagDefinition> {
        self.definitions
            .iter()
            .find(|definition| definition.tag_id == tag_id)
    }
}

/// Decode a named object that holds a tag set.
///
/// Returns `None` for another object and for bytes that do not match the
/// observed layout exactly. The object keeps its raw bytes in both cases.
#[must_use]
pub fn decode_v8_tag_set(object: &V8RawObject) -> Option<V8TagSet> {
    if object.element_type != TAG_SET_ELEMENT_TYPE {
        return None;
    }
    decode_v8_tag_set_data(
        object.element_id?,
        object.primary_bytes(),
        object.attribute_bytes(),
    )
}

/// Decode a tag set from the primary bytes and the attribute bytes of its
/// named object.
///
/// The decoder accepts only the observed layout: the code at `0x28`, the
/// byte length of the definitions stored twice, and definitions that use
/// exactly that many bytes, with at most one zero byte of padding after
/// them. A definition is the name, the tag ID, the prompt, the value type,
/// two unresolved words, a zero byte and the default. A value type other
/// than text or integer makes the whole set undecodable, and so do more
/// than [`V8_MAX_TAG_SET_BYTES`] definition bytes.
///
/// The name linkage in `attributes` is decoded with the default limits of
/// the reader, not with the options of a read, and a linkage that does not
/// decode leaves the set undecoded. The strings of the definitions are held
/// by the limit of the set alone.
#[must_use]
pub fn decode_v8_tag_set_data(
    element_id: u64,
    primary: &[u8],
    attributes: &[u8],
) -> Option<V8TagSet> {
    let size = declared_size(primary)?;
    let mut cursor = Cursor {
        data: primary.get(TAG_SET_DEFINITIONS_OFFSET..)?.get(..size)?,
        offset: 0,
    };
    let mut definitions = Vec::new();
    while cursor.offset < cursor.data.len() {
        let name = cursor.text()?;
        let tag_id = cursor.word()?;
        let prompt = cursor.text()?;
        let value_type = cursor.word()?;
        cursor.take(TAG_SET_UNRESOLVED_BYTES)?;
        if cursor.take(1)? != [0] {
            return None;
        }
        let default = match value_type {
            V8_TAG_VALUE_TEXT => V8TagValue::Text(cursor.text()?),
            V8_TAG_VALUE_INTEGER => V8TagValue::Integer(integer(cursor.take(4)?)?),
            _ => return None,
        };
        definitions.push(V8TagDefinition {
            name,
            tag_id,
            prompt,
            value_type,
            default,
        });
    }
    let linkages = decode_v8_linkages(attributes, V8ScanOptions::default(), "tag set").ok()?;
    let name = linkages
        .iter()
        .find(|linkage| linkage.property_id == Some(1))
        .and_then(|linkage| linkage.property_text.clone())
        .unwrap_or_default();
    Some(V8TagSet {
        element_id,
        name,
        definitions,
    })
}

/// Decode the tag sets of a scanned document, in stored order.
///
/// A named object that carries the tag-set code but does not decode is not
/// listed; [`V8RawDocument::tag_set_objects`] still returns it. One call
/// reads at most [`V8_MAX_TAG_SET_BYTES`] bytes in sum; see
/// [`decode_v8_tag_set_parts`] for what is counted.
#[must_use]
pub fn decode_v8_tag_sets(raw: &V8RawDocument) -> Vec<V8TagSet> {
    decode_v8_tag_set_parts(raw.tag_set_objects().filter_map(|object| {
        Some((
            object.element_id?,
            object.primary_bytes(),
            object.attribute_bytes(),
        ))
    }))
}

/// Decode tag sets from the element ID, the primary bytes and the attribute
/// bytes of their named objects, in the given order.
///
/// One call reads at most [`V8_MAX_TAG_SET_BYTES`] bytes in sum. An object
/// costs the definition bytes it declares plus its attribute bytes, which
/// hold its name. An object that cannot decode within the limit is passed
/// over and costs nothing: its two stored sizes differ, they declare more
/// than the limit, its primary bytes do not end with the declared
/// definition bytes, or its own cost is over the limit. The first other
/// object that would pass the sum ends the call, and it and the objects
/// after it are not listed.
#[must_use]
pub fn decode_v8_tag_set_parts<'a>(
    parts: impl IntoIterator<Item = (u64, &'a [u8], &'a [u8])>,
) -> Vec<V8TagSet> {
    let mut budget = V8_MAX_TAG_SET_BYTES;
    let mut sets = Vec::new();
    for (element_id, primary, attributes) in parts {
        let Some(cost) = declared_size(primary)
            .map(|size| size.saturating_add(attributes.len()))
            .filter(|cost| *cost <= V8_MAX_TAG_SET_BYTES)
        else {
            continue;
        };
        if cost > budget {
            break;
        }
        budget -= cost;
        sets.extend(decode_v8_tag_set_data(element_id, primary, attributes));
    }
    sets
}

/// The declared byte length of the definitions, when the object carries the
/// tag-set code, stores the length twice, the length is within the limit of
/// one set, and the primary bytes end with that many definition bytes (one
/// zero byte may follow them). An object that declares bytes it does not
/// hold has no size, so it is never charged for them.
fn declared_size(primary: &[u8]) -> Option<usize> {
    if !holds_tag_set_code(primary) {
        return None;
    }
    let size = read_u32(primary, TAG_SET_SIZE_OFFSET)?;
    if read_u32(primary, TAG_SET_SIZE_OFFSET + 4)? != size {
        return None;
    }
    let size = usize::try_from(size).ok()?;
    let end = TAG_SET_DEFINITIONS_OFFSET.checked_add(size)?;
    (size <= V8_MAX_TAG_SET_BYTES && matches!(primary.get(end..)?, [] | [0])).then_some(size)
}

impl V8RawDocument {
    /// The named objects that carry the tag-set code, decodable or not.
    pub fn tag_set_objects(&self) -> impl Iterator<Item = &V8RawObject> {
        self.named_pages
            .iter()
            .flat_map(|page| page.objects.iter())
            .filter(|object| {
                object.element_type == TAG_SET_ELEMENT_TYPE
                    && holds_tag_set_code(object.primary_bytes())
            })
    }
}

fn holds_tag_set_code(primary: &[u8]) -> bool {
    primary.get(TAG_SET_CODE_OFFSET..TAG_SET_CODE_OFFSET + V8_TAG_SET_CODE.len())
        == Some(&V8_TAG_SET_CODE[..])
}

fn integer(bytes: &[u8]) -> Option<i32> {
    Some(i32::from_le_bytes(bytes.try_into().ok()?))
}

struct Cursor<'a> {
    data: &'a [u8],
    offset: usize,
}

impl Cursor<'_> {
    fn take(&mut self, length: usize) -> Option<&[u8]> {
        let end = self.offset.checked_add(length)?;
        let bytes = self.data.get(self.offset..end)?;
        self.offset = end;
        Some(bytes)
    }

    fn word(&mut self) -> Option<u16> {
        let value = read_u16(self.data, self.offset)?;
        self.offset += 2;
        Some(value)
    }

    /// 8-bit text up to its closing zero byte.
    fn text(&mut self) -> Option<String> {
        let rest = self.data.get(self.offset..)?;
        let length = rest.iter().position(|byte| *byte == 0)?;
        self.offset += length + 1;
        Some(decode_windows_1252(&rest[..length]))
    }
}

/// The fields of one tag element that do not depend on its model.
pub(super) struct TagFields {
    pub(super) tag_id: u16,
    pub(super) value_type: u16,
    pub(super) value_bytes: Arc<[u8]>,
    pub(super) value: Option<V8TagValue>,
    pub(super) tag_set_element_id: Option<u64>,
    pub(super) host_element_id: Option<u64>,
    /// Sum of the two stored XY pairs, when all four values are finite.
    pub(super) placement_uor: Option<[f64; 2]>,
}

/// Read a tag element, or return `None` when the stored value does not fit
/// the primary bytes or is longer than `max_value_bytes`.
pub(super) fn decode_tag_fields(
    primary: &[u8],
    linkages: &[V8Linkage],
    max_value_bytes: usize,
) -> Option<TagFields> {
    let tag_id = read_u16(primary, TAG_ID_OFFSET)?;
    let value_type = read_u16(primary, TAG_VALUE_TYPE_OFFSET)?;
    let size = usize::try_from(read_u32(primary, TAG_VALUE_SIZE_OFFSET)?).ok()?;
    if size > max_value_bytes {
        return None;
    }
    let code_page = read_u32(primary, TAG_CODE_PAGE_OFFSET)?;
    let value_bytes = primary.get(TAG_VALUE_OFFSET..TAG_VALUE_OFFSET.checked_add(size)?)?;
    let placement = [
        read_f64(primary, TAG_ANCHOR_OFFSET)? + read_f64(primary, TAG_OFFSET_OFFSET)?,
        read_f64(primary, TAG_ANCHOR_OFFSET + 8)? + read_f64(primary, TAG_OFFSET_OFFSET + 8)?,
    ];
    let placement_uor = placement
        .iter()
        .all(|value| value.is_finite())
        .then_some(placement);
    Some(TagFields {
        tag_id,
        value_type,
        value_bytes: Arc::from(value_bytes),
        value: decode_tag_value(value_type, value_bytes, code_page),
        tag_set_element_id: linkages.iter().find_map(tag_set_reference),
        host_element_id: linkages.iter().find_map(host_reference),
        placement_uor,
    })
}

/// Text needs its closing zero byte and no other zero byte. Bytes above
/// `0x7f` are read as Windows-1252 only when the tag stores that code page
/// number. An integer needs exactly four bytes.
fn decode_tag_value(value_type: u16, bytes: &[u8], code_page: u32) -> Option<V8TagValue> {
    match value_type {
        V8_TAG_VALUE_TEXT => {
            let (last, text) = bytes.split_last()?;
            if *last != 0 || text.contains(&0) {
                return None;
            }
            if !text.is_ascii() && code_page != WINDOWS_1252 {
                return None;
            }
            Some(V8TagValue::Text(decode_windows_1252(text)))
        }
        V8_TAG_VALUE_INTEGER => integer(bytes).map(V8TagValue::Integer),
        _ => None,
    }
}

fn reference_bytes(linkage: &V8Linkage, length: usize) -> Option<&[u8]> {
    let bytes = linkage.raw_bytes.as_ref();
    (linkage.kind_code == Some(REFERENCE_LINKAGE_KIND) && bytes.len() == length).then_some(bytes)
}

fn tag_set_reference(linkage: &V8Linkage) -> Option<u64> {
    let bytes = reference_bytes(linkage, SET_LINKAGE_BYTES)?;
    if bytes[4..SET_LINKAGE_ID_OFFSET] != SET_LINKAGE_HEAD {
        return None;
    }
    read_u64(bytes, SET_LINKAGE_ID_OFFSET)
}

fn host_reference(linkage: &V8Linkage) -> Option<u64> {
    let bytes = reference_bytes(linkage, HOST_LINKAGE_BYTES)?;
    if bytes[4..4 + HOST_LINKAGE_HEAD.len()] != HOST_LINKAGE_HEAD
        || bytes[HOST_LINKAGE_ROOT_OFFSET..HOST_LINKAGE_ID_OFFSET] != HOST_LINKAGE_ROOT
    {
        return None;
    }
    read_u64(bytes, HOST_LINKAGE_ID_OFFSET)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// The 24-byte linkage that names the tag set of a tag.
    pub(crate) fn tag_set_linkage(set_id: u64) -> Vec<u8> {
        let mut bytes = vec![0x0b, 0x10, 0xd0, 0x56];
        bytes.extend(SET_LINKAGE_HEAD);
        bytes.extend(set_id.to_le_bytes());
        bytes.extend([0; 4]);
        bytes
    }

    /// The 56-byte linkage that names the host of a tag. `writer` are the
    /// two bytes that the observed writers store differently.
    pub(crate) fn host_linkage(host_id: u64, writer: [u8; 2]) -> Vec<u8> {
        let mut bytes = vec![0x1b, 0x10, 0xd0, 0x56];
        bytes.extend(HOST_LINKAGE_HEAD);
        bytes.extend(writer);
        bytes.extend(HOST_LINKAGE_ROOT);
        bytes.extend(host_id.to_le_bytes());
        bytes.resize(HOST_LINKAGE_BYTES, 0);
        bytes
    }

    /// The fields of a tag that a test chooses. Everything else is zero.
    #[derive(Clone, Copy)]
    pub(crate) struct TagParts<'a> {
        pub(crate) tag_id: u16,
        pub(crate) value_type: u16,
        pub(crate) value: &'a [u8],
        pub(crate) code_page: u32,
        pub(crate) anchor: [f64; 2],
        pub(crate) offset: [f64; 2],
        /// Bytes that one writer stores after the value.
        pub(crate) trailer: &'a [u8],
    }

    impl Default for TagParts<'_> {
        fn default() -> Self {
            Self {
                tag_id: 1,
                value_type: V8_TAG_VALUE_TEXT,
                value: b"\0",
                code_page: 0,
                anchor: [0.0; 2],
                offset: [0.0; 2],
                trailer: &[],
            }
        }
    }

    /// Primary bytes of a tag element, padded to an even length. The common
    /// header fields are zero.
    pub(crate) fn tag_primary(parts: &TagParts<'_>) -> Vec<u8> {
        let mut bytes = vec![0; TAG_VALUE_OFFSET];
        let mut put = |offset: usize, value: &[u8]| {
            bytes[offset..offset + value.len()].copy_from_slice(value);
        };
        put(TAG_ANCHOR_OFFSET, &parts.anchor[0].to_le_bytes());
        put(TAG_ANCHOR_OFFSET + 8, &parts.anchor[1].to_le_bytes());
        put(TAG_OFFSET_OFFSET, &parts.offset[0].to_le_bytes());
        put(TAG_OFFSET_OFFSET + 8, &parts.offset[1].to_le_bytes());
        put(TAG_ID_OFFSET, &parts.tag_id.to_le_bytes());
        put(TAG_VALUE_TYPE_OFFSET, &parts.value_type.to_le_bytes());
        put(
            TAG_VALUE_SIZE_OFFSET,
            &u32::try_from(parts.value.len()).unwrap().to_le_bytes(),
        );
        put(TAG_CODE_PAGE_OFFSET, &parts.code_page.to_le_bytes());
        bytes.extend(parts.value);
        bytes.extend(parts.trailer);
        bytes.resize(bytes.len().div_ceil(2) * 2, 0);
        bytes
    }

    /// One definition of a tag set. `unresolved` are the two words that the
    /// observed writers fill differently; `default` is the stored default.
    pub(crate) fn definition_bytes(
        name: &str,
        tag_id: u16,
        prompt: &str,
        value_type: u16,
        unresolved: [u16; 2],
        default: &[u8],
    ) -> Vec<u8> {
        let mut bytes = name.as_bytes().to_vec();
        bytes.push(0);
        bytes.extend(tag_id.to_le_bytes());
        bytes.extend(prompt.as_bytes());
        bytes.push(0);
        bytes.extend(value_type.to_le_bytes());
        bytes.extend(unresolved[0].to_le_bytes());
        bytes.extend(unresolved[1].to_le_bytes());
        bytes.push(0);
        bytes.extend(default);
        bytes
    }

    /// Primary bytes of a tag-set object around the given definition bytes,
    /// padded to an even length. The object header fields are zero.
    pub(crate) fn tag_set_primary(definitions: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; TAG_SET_DEFINITIONS_OFFSET];
        bytes[TAG_SET_CODE_OFFSET..TAG_SET_CODE_OFFSET + 8].copy_from_slice(&V8_TAG_SET_CODE);
        let size = u32::try_from(definitions.len()).unwrap().to_le_bytes();
        bytes[TAG_SET_SIZE_OFFSET..TAG_SET_SIZE_OFFSET + 4].copy_from_slice(&size);
        bytes[TAG_SET_SIZE_OFFSET + 4..TAG_SET_SIZE_OFFSET + 8].copy_from_slice(&size);
        bytes.extend(definitions);
        bytes.resize(bytes.len().div_ceil(2) * 2, 0);
        bytes
    }

    /// A name linkage with plain 8-bit text.
    pub(crate) fn plain_name_linkage(name: &str) -> Vec<u8> {
        let mut bytes = vec![0, 0x10, 0xd2, 0x56];
        bytes.extend(1_u32.to_le_bytes());
        bytes.extend(u32::try_from(name.len()).unwrap().to_le_bytes());
        bytes.extend(name.as_bytes());
        bytes.resize(bytes.len().div_ceil(4) * 4, 0);
        bytes[0] = u8::try_from(bytes.len() / 2 - 1).unwrap();
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{
        definition_bytes, host_linkage, plain_name_linkage, tag_primary, tag_set_linkage,
        tag_set_primary, TagParts,
    };
    use super::*;

    fn linkages(bytes: &[u8]) -> Vec<V8Linkage> {
        decode_v8_linkages(bytes, V8ScanOptions::default(), "test").unwrap()
    }

    /// Two definitions as one writer stores them and two as the other does:
    /// the unresolved words differ and so does the padding.
    fn sample_definitions() -> Vec<u8> {
        [
            definition_bytes("Variety", 1, "Kind of pear", 1, [3, 1], b"Oak\0"),
            definition_bytes("Yield", 2, "", 3, [4, 3], &(-7_i32).to_le_bytes()),
            definition_bytes("Note", 3, "Say why", 1, [1, 0], b"\0"),
            definition_bytes("Caf?", 4, "", 1, [5, 0], b"none\0"),
        ]
        .concat()
    }

    #[test]
    fn decodes_the_definitions_of_a_tag_set() {
        let mut definitions = sample_definitions();
        // a name with a byte above 0x7f is read as Windows-1252
        let position = definitions.iter().position(|byte| *byte == b'?').unwrap();
        definitions[position] = 0xe9;
        assert_eq!(definitions.len() % 2, 1);
        let primary = tag_set_primary(&definitions);
        assert_eq!(
            primary.len(),
            TAG_SET_DEFINITIONS_OFFSET + definitions.len() + 1
        );
        let set = decode_v8_tag_set_data(77, &primary, &plain_name_linkage("Grove")).unwrap();
        assert_eq!((set.element_id, set.name.as_str()), (77, "Grove"));
        assert_eq!(
            set.definitions,
            [
                V8TagDefinition {
                    name: "Variety".to_owned(),
                    tag_id: 1,
                    prompt: "Kind of pear".to_owned(),
                    value_type: V8_TAG_VALUE_TEXT,
                    default: V8TagValue::Text("Oak".to_owned()),
                },
                V8TagDefinition {
                    name: "Yield".to_owned(),
                    tag_id: 2,
                    prompt: String::new(),
                    value_type: V8_TAG_VALUE_INTEGER,
                    default: V8TagValue::Integer(-7),
                },
                V8TagDefinition {
                    name: "Note".to_owned(),
                    tag_id: 3,
                    prompt: "Say why".to_owned(),
                    value_type: V8_TAG_VALUE_TEXT,
                    default: V8TagValue::Text(String::new()),
                },
                V8TagDefinition {
                    name: "Caf\u{e9}".to_owned(),
                    tag_id: 4,
                    prompt: String::new(),
                    value_type: V8_TAG_VALUE_TEXT,
                    default: V8TagValue::Text("none".to_owned()),
                },
            ]
        );
        assert_eq!(set.definition(2).unwrap().name, "Yield");
        assert!(set.definition(5).is_none());

        // an even definition length has no padding; no name linkage, no name
        let last = definition_bytes("X", 5, "", 1, [0, 0], b"\0");
        let even = [definitions.as_slice(), last.as_slice()].concat();
        assert_eq!(even.len() % 2, 0);
        let set = decode_v8_tag_set_data(78, &tag_set_primary(&even), &[]).unwrap();
        assert_eq!((set.name.as_str(), set.definitions.len()), ("", 5));
        // a set without definitions
        let empty = decode_v8_tag_set_data(79, &tag_set_primary(&[]), &[]).unwrap();
        assert!(empty.definitions.is_empty());
    }

    #[test]
    fn one_call_counts_names_too_and_leaves_out_what_cannot_decode() {
        let primary = tag_set_primary(&sample_definitions());
        let ids = |parts: &[(u64, &[u8], &[u8])]| {
            decode_v8_tag_set_parts(parts.iter().copied())
                .iter()
                .map(|set| set.element_id)
                .collect::<Vec<_>>()
        };
        // An object that declares more than the limit and one whose two
        // sizes differ cannot decode. They cost nothing and do not end the
        // call: the set after them is listed.
        let over = u32::try_from(V8_MAX_TAG_SET_BYTES + 1)
            .unwrap()
            .to_le_bytes();
        let mut oversized = primary.clone();
        oversized[TAG_SET_SIZE_OFFSET..TAG_SET_SIZE_OFFSET + 4].copy_from_slice(&over);
        oversized[TAG_SET_SIZE_OFFSET + 4..TAG_SET_SIZE_OFFSET + 8].copy_from_slice(&over);
        let mut unequal = primary.clone();
        unequal[TAG_SET_SIZE_OFFSET] ^= 1;
        assert_eq!(
            ids(&[(1, &oversized, &[]), (2, &unequal, &[]), (3, &primary, &[])]),
            [3]
        );

        // The attribute bytes count. Two sets without definitions whose
        // attribute bytes fill the limit leave no room for a third set.
        let empty = tag_set_primary(&[]);
        let half = vec![0; V8_MAX_TAG_SET_BYTES / 2];
        assert_eq!(
            ids(&[(1, &empty, &half), (2, &empty, &half), (3, &primary, &[])]),
            [1, 2]
        );
        assert_eq!(ids(&[(1, &empty, &half), (3, &primary, &[])]), [1, 3]);
        // An object whose own cost is over the limit is passed over.
        let more = vec![0; V8_MAX_TAG_SET_BYTES + 1];
        assert_eq!(ids(&[(1, &empty, &more), (3, &primary, &[])]), [3]);
    }

    #[test]
    fn a_tag_set_that_declares_bytes_it_does_not_hold_costs_nothing() {
        let primary = tag_set_primary(&sample_definitions());
        let ids = |parts: &[(u64, &[u8], &[u8])]| {
            decode_v8_tag_set_parts(parts.iter().copied())
                .iter()
                .map(|set| set.element_id)
                .collect::<Vec<_>>()
        };
        // 60 bytes: the head alone, with the whole limit declared twice.
        let declare = |primary: &mut Vec<u8>, size: usize| {
            let size = u32::try_from(size).unwrap().to_le_bytes();
            primary[TAG_SET_SIZE_OFFSET..TAG_SET_SIZE_OFFSET + 4].copy_from_slice(&size);
            primary[TAG_SET_SIZE_OFFSET + 4..TAG_SET_SIZE_OFFSET + 8].copy_from_slice(&size);
        };
        let mut head_only = tag_set_primary(&[]);
        assert_eq!(head_only.len(), 60);
        declare(&mut head_only, V8_MAX_TAG_SET_BYTES);
        assert!(decode_v8_tag_set_data(1, &head_only, &[]).is_none());
        // It is not charged for the bytes it does not hold: the valid sets
        // after it are listed, also after many such objects.
        assert_eq!(ids(&[(1, &head_only, &[]), (2, &primary, &[])]), [2]);
        let mut parts = vec![(1, head_only.as_slice(), &[][..]); 100];
        parts.push((2, &primary, &[]));
        parts.push((3, &primary, &[]));
        assert_eq!(ids(&parts), [2, 3]);

        // One byte less or two bytes more than the object holds: no size.
        let held = sample_definitions().len();
        for declared in [held - 1, held + 2, held + 100] {
            let mut wrong = primary.clone();
            declare(&mut wrong, declared);
            assert!(declared_size(&wrong).is_none(), "{declared}");
            assert_eq!(ids(&[(1, &wrong, &[]), (2, &primary, &[])]), [2]);
        }
        assert_eq!(declared_size(&primary), Some(held));
    }

    #[test]
    fn a_name_linkage_that_does_not_decode_leaves_the_set_undecoded() {
        let primary = tag_set_primary(&sample_definitions());
        assert!(decode_v8_tag_set_data(1, &primary, &plain_name_linkage("Grove")).is_some());
        // a property linkage that declares more text than any limit allows
        let mut linkage = ((crate::v8::V8_PROPERTY_LINKAGE_KIND << 8) | 5)
            .to_le_bytes()
            .to_vec();
        linkage.extend(1_u32.to_le_bytes());
        linkage.extend(0x7fff_ffff_u32.to_le_bytes());
        assert!(decode_v8_tag_set_data(1, &primary, &linkage).is_none());
    }

    #[test]
    fn a_tag_set_that_does_not_match_the_layout_stays_raw() {
        let definitions = sample_definitions();
        let good = tag_set_primary(&definitions);
        let decode = |primary: &[u8]| decode_v8_tag_set_data(1, primary, &[]);
        assert!(decode(&good).is_some());

        // another code
        for offset in TAG_SET_CODE_OFFSET..TAG_SET_CODE_OFFSET + 8 {
            let mut changed = good.clone();
            changed[offset] ^= 1;
            assert!(decode(&changed).is_none(), "code byte {offset:#x}");
        }
        // the two stored sizes differ
        let mut changed = good.clone();
        changed[TAG_SET_SIZE_OFFSET + 4] ^= 1;
        assert!(decode(&changed).is_none());
        // the size is one definition short: bytes remain after the definitions
        let shorter =
            definitions.len() - definition_bytes("Caf?", 4, "", 1, [5, 0], b"none\0").len();
        let mut changed = good.clone();
        for offset in [TAG_SET_SIZE_OFFSET, TAG_SET_SIZE_OFFSET + 4] {
            changed[offset..offset + 4].copy_from_slice(&(shorter as u32).to_le_bytes());
        }
        assert!(decode(&changed).is_none());
        // the size runs past the primary bytes
        let mut changed = good.clone();
        for offset in [TAG_SET_SIZE_OFFSET, TAG_SET_SIZE_OFFSET + 4] {
            changed[offset..offset + 4]
                .copy_from_slice(&(definitions.len() as u32 + 2).to_le_bytes());
        }
        assert!(decode(&changed).is_none());
        // a padding byte that is not zero, and two padding bytes
        let mut changed = good.clone();
        *changed.last_mut().unwrap() = 1;
        assert!(decode(&changed).is_none());
        assert!(decode(&[good.as_slice(), &[0_u8, 0][..]].concat()).is_none());
        // truncated at every length
        for end in 0..good.len() - 1 {
            assert!(decode(&good[..end]).is_none(), "prefix {end}");
        }

        let with = |definition: Vec<u8>| decode(&tag_set_primary(&definition));
        assert!(with(definition_bytes("A", 1, "", 1, [0, 0], b"\0")).is_some());
        // a value type that is neither text nor integer
        assert!(with(definition_bytes("A", 1, "", 5, [0, 0], b"\0")).is_none());
        assert!(with(definition_bytes("A", 1, "", 0, [0, 0], b"\0")).is_none());
        // the byte before the default is not zero
        let mut changed = definition_bytes("A", 1, "", 1, [0, 0], b"\0");
        let position = changed.len() - 2;
        changed[position] = 1;
        assert!(with(changed).is_none());
        // a text default without its closing zero byte, a short integer
        assert!(with(definition_bytes("A", 1, "", 1, [0, 0], b"abc")).is_none());
        assert!(with(definition_bytes("A", 1, "", 3, [0, 0], &[1, 0, 0])).is_none());
        // bytes after the last definition that are no definition
        let mut changed = definition_bytes("A", 1, "", 1, [0, 0], b"\0");
        changed.extend(b"B\0");
        assert!(with(changed).is_none());
    }

    #[test]
    fn decodes_tag_values_of_the_two_observed_types() {
        let fields =
            |parts: TagParts<'_>| decode_tag_fields(&tag_primary(&parts), &[], usize::MAX).unwrap();
        let value = |parts: TagParts<'_>| fields(parts).value;
        let text = |value: &str| Some(V8TagValue::Text(value.to_owned()));

        let tag = fields(TagParts {
            tag_id: 9,
            value: b"Oak\0",
            ..TagParts::default()
        });
        assert_eq!((tag.tag_id, tag.value_type), (9, V8_TAG_VALUE_TEXT));
        assert_eq!(tag.value_bytes.as_ref(), b"Oak\0");
        assert_eq!(tag.value, text("Oak"));
        assert_eq!((tag.tag_set_element_id, tag.host_element_id), (None, None));
        // the empty text, and the bytes that one writer stores after the value
        assert_eq!(value(TagParts::default()), text(""));
        assert_eq!(
            value(TagParts {
                value: b"Oak\0",
                trailer: &[7; 11],
                ..TagParts::default()
            }),
            text("Oak")
        );
        // text needs its closing zero byte and no zero byte before it
        for bytes in [&b""[..], b"Oak", b"O\0k\0"] {
            let tag = fields(TagParts {
                value: bytes,
                ..TagParts::default()
            });
            assert_eq!(tag.value, None, "{bytes:?}");
            assert_eq!(tag.value_bytes.as_ref(), bytes);
        }
        // a byte above 0x7f is text only with the Windows-1252 code page word
        let high = TagParts {
            value: b"1\xbd\x80\0",
            ..TagParts::default()
        };
        assert_eq!(value(high), None);
        assert_eq!(
            value(TagParts {
                code_page: 932,
                ..high
            }),
            None
        );
        assert_eq!(
            value(TagParts {
                code_page: 1252,
                ..high
            }),
            text("1\u{bd}\u{20ac}")
        );

        let integer = |bytes: &'static [u8]| {
            value(TagParts {
                value_type: V8_TAG_VALUE_INTEGER,
                value: bytes,
                ..TagParts::default()
            })
        };
        assert_eq!(integer(&[0x2a, 0, 0, 0]), Some(V8TagValue::Integer(42)));
        assert_eq!(
            integer(&[0xfe, 0xff, 0xff, 0xff]),
            Some(V8TagValue::Integer(-2))
        );
        assert_eq!(integer(&[0x2a, 0]), None);
        assert_eq!(integer(&[0x2a, 0, 0, 0, 0]), None);
        // every other value type keeps its bytes and has no value
        for value_type in [0, 2, 4, 5] {
            let tag = fields(TagParts {
                value_type,
                value: b"Oak\0",
                ..TagParts::default()
            });
            assert_eq!((tag.value_type, tag.value), (value_type, None));
            assert_eq!(tag.value_bytes.as_ref(), b"Oak\0");
        }
    }

    #[test]
    fn a_tag_whose_value_does_not_fit_is_not_read() {
        let primary = tag_primary(&TagParts {
            value: b"Oak\0",
            ..TagParts::default()
        });
        assert!(decode_tag_fields(&primary, &[], usize::MAX).is_some());
        for end in 0..primary.len() {
            assert!(
                decode_tag_fields(&primary[..end], &[], usize::MAX).is_none(),
                "prefix {end}"
            );
        }
        let mut longer = primary.clone();
        longer[TAG_VALUE_SIZE_OFFSET] = 5;
        assert!(decode_tag_fields(&longer, &[], usize::MAX).is_none());
        let mut huge = primary.clone();
        huge[TAG_VALUE_SIZE_OFFSET..TAG_VALUE_SIZE_OFFSET + 4].copy_from_slice(&[0xff; 4]);
        assert!(decode_tag_fields(&huge, &[], usize::MAX).is_none());
        // a value of four bytes is read up to a limit of four bytes
        assert!(decode_tag_fields(&primary, &[], 4).is_some());
        assert!(decode_tag_fields(&primary, &[], 3).is_none());
    }

    #[test]
    fn a_tag_set_holds_at_most_the_stated_number_of_definition_bytes() {
        // one definition whose default text fills the set up to a length
        let fill = |length: usize| {
            let head = definition_bytes("A", 1, "", 1, [0, 0], b"");
            let mut default = vec![b'x'; length - head.len() - 1];
            default.push(0);
            definition_bytes("A", 1, "", 1, [0, 0], &default)
        };
        let decode =
            |definitions: &[u8]| decode_v8_tag_set_data(1, &tag_set_primary(definitions), &[]);
        let at_limit = fill(V8_MAX_TAG_SET_BYTES);
        assert_eq!(at_limit.len(), 1_048_576);
        assert_eq!(decode(&at_limit).unwrap().definitions.len(), 1);
        assert!(decode(&fill(V8_MAX_TAG_SET_BYTES + 1)).is_none());
        // The smallest definition is 12 bytes. A hostile set of them cannot
        // ask for more definitions than the limit has room for.
        let smallest = definition_bytes("", 0, "", 1, [0, 0], b"\0");
        assert_eq!(smallest.len(), 12);
        assert_eq!(
            decode(&smallest.repeat(87_381)).unwrap().definitions.len(),
            87_381
        );
        assert!(decode(&smallest.repeat(87_382)).is_none());
    }

    #[test]
    fn the_placement_is_the_sum_of_the_two_stored_pairs() {
        let placement = |anchor, offset| {
            let parts = TagParts {
                anchor,
                offset,
                ..TagParts::default()
            };
            decode_tag_fields(&tag_primary(&parts), &[], usize::MAX)
                .unwrap()
                .placement_uor
        };
        assert_eq!(placement([100.0, 200.0], [0.0, 0.0]), Some([100.0, 200.0]));
        assert_eq!(
            placement([100.0, 200.0], [5.0, -20.0]),
            Some([105.0, 180.0])
        );
        assert_eq!(placement([f64::NAN, 0.0], [0.0, 0.0]), None);
        assert_eq!(placement([0.0, 0.0], [0.0, f64::INFINITY]), None);
        assert_eq!(placement([f64::MAX, 0.0], [f64::MAX, 0.0]), None);
    }

    #[test]
    fn reads_the_tag_set_and_the_host_from_their_linkages() {
        let primary = tag_primary(&TagParts::default());
        let ids = |attributes: &[u8]| {
            let tag = decode_tag_fields(&primary, &linkages(attributes), usize::MAX).unwrap();
            (tag.tag_set_element_id, tag.host_element_id)
        };
        let set = tag_set_linkage(0x1_0000_0321);
        assert_eq!(set.len(), SET_LINKAGE_BYTES);
        assert_eq!(ids(&set), (Some(0x1_0000_0321), None));
        // the two observed writers differ in two bytes of the host linkage
        for writer in [[0x00, 0x0a], [0x02, 0x09]] {
            let host = host_linkage(900, writer);
            assert_eq!(host.len(), HOST_LINKAGE_BYTES);
            let both = (Some(0x1_0000_0321), Some(900));
            assert_eq!(ids(&[set.as_slice(), host.as_slice()].concat()), both);
            assert_eq!(ids(&[host.as_slice(), set.as_slice()].concat()), both);
            assert_eq!(ids(&host), (None, Some(900)));
        }
        // a name linkage first; the first matching linkage of each kind wins
        let more = [
            plain_name_linkage("n"),
            set.clone(),
            tag_set_linkage(5),
            host_linkage(900, [0, 0x0a]),
            host_linkage(6, [0, 0x0a]),
        ]
        .concat();
        assert_eq!(ids(&more), (Some(0x1_0000_0321), Some(900)));

        // any other head, another linkage kind or another length is no reference
        for offset in 4..SET_LINKAGE_ID_OFFSET {
            let mut changed = set.clone();
            changed[offset] ^= 1;
            assert_eq!(ids(&changed), (None, None), "set linkage byte {offset}");
        }
        let host = host_linkage(900, [0x00, 0x0a]);
        for offset in (4..8).chain(HOST_LINKAGE_ROOT_OFFSET..HOST_LINKAGE_ID_OFFSET) {
            let mut changed = host.clone();
            changed[offset] ^= 1;
            assert_eq!(ids(&changed), (None, None), "host linkage byte {offset}");
        }
        for mut changed in [set.clone(), host.clone()] {
            changed[2] = 0xd1;
            assert_eq!(ids(&changed), (None, None));
        }
        let mut longer = set.clone();
        longer.extend([0; 4]);
        longer[0] = 0x0d;
        assert_eq!(ids(&longer), (None, None));
        let mut shorter = host;
        shorter.truncate(52);
        shorter[0] = 0x19;
        assert_eq!(ids(&shorter), (None, None));
    }
}
