use std::sync::Arc;

use super::auxiliary::V8AuxiliaryEntry;
use super::raw::read_u32;

/// Type word of the auxiliary entries that hold one item instance.
pub const V8_ITEM_INSTANCE_ENTRY_TYPE: u32 = 0x56ff_0000;

/// Most stored bytes of item entries that one call returns as instances.
///
/// An instance never copies a string of its string table. The table is
/// decoded once, and the class name, every name and every value is a shared
/// handle to one of its strings. The decoded size of an instance is
/// therefore at most a fixed multiple of its stored size (the worked numbers
/// are in `docs/v8/FORMAT_NOTES.md`), and this limit holds what one call
/// returns to a fixed size. An item entry whose data is larger than what the
/// call has left is not decoded.
pub const V8_MAX_ITEM_BYTES: usize = 1024 * 1024;

const HEAD: [u8; 5] = [0x00, 0x00, 0x01, 0x01, 0x15];
const SCHEMA_ID_OFFSET: usize = HEAD.len();
const SIGNATURE_OFFSET: usize = SCHEMA_ID_OFFSET + 4;
const SIGNATURE: [u8; 18] = [
    0x01, b'B', b'E', b'C', b'X', b'M', b'L', 0x00, 0xff, 0x0a, 0x0d, 0x01, 0x00, 0x00, 0x03, 0x00,
    0x00, 0x30,
];
const STRING_COUNT_OFFSET: usize = SIGNATURE_OFFSET + SIGNATURE.len();

/// Counts, lengths and string numbers are single bytes. Only values below
/// this limit were observed, so a larger byte is not read as a number.
const SINGLE_BYTE_LIMIT: u8 = 0x80;

const STRING_ONE_BYTE: u8 = 0xfa;
const STRING_UTF16: u8 = 0xfb;

const TOKEN_CHILD: u8 = 0x02;
const TOKEN_ROOT: u8 = 0x03;
const TOKEN_END: u8 = 0x04;
const TOKEN_ATTRIBUTE: u8 = 0x05;
const TOKEN_CONTENT: u8 = 0x06;
const TOKEN_VALUE: u8 = 0x11;

/// Stored bytes of one attribute: the token, the name number, the value
/// token and the value number.
const ATTRIBUTE_BYTES: usize = 4;
/// Stored bytes of one property: an attribute and the end token.
const PROPERTY_BYTES: usize = 5;

/// One name with its text value, exactly as stored.
///
/// Both are handles to strings of the string table of the instance. Equal
/// string numbers share one string, so a clone of a handle copies no text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V8ItemValue {
    pub name: Arc<str>,
    pub value: Arc<str>,
}

/// One item instance attached to an element.
///
/// The instance is a small XML element stored in a tokenized binary form: a
/// root element named after the item class, attributes on the root, and one
/// child element with text for each property. Names and values are returned
/// as stored; no type conversion and no name unescaping is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V8ItemInstance {
    /// Identifier of the auxiliary entry that holds the instance.
    pub entry_id: u32,
    /// Element ID of the named object that holds the schema text.
    pub schema_element_id: u32,
    /// Name of the root element: a handle to a string of the string table.
    pub class_name: Arc<str>,
    /// Attributes of the root element in stored order.
    pub attributes: Vec<V8ItemValue>,
    /// Child elements in stored order: property name and text value.
    pub properties: Vec<V8ItemValue>,
}

impl V8ItemInstance {
    /// Value of the first root attribute with this name.
    #[must_use]
    pub fn attribute(&self, name: &str) -> Option<&str> {
        first_value(&self.attributes, name)
    }

    /// Value of the first property with this name.
    #[must_use]
    pub fn property(&self, name: &str) -> Option<&str> {
        first_value(&self.properties, name)
    }

    /// The `xmlns` attribute: schema name and version of the item class.
    #[must_use]
    pub fn schema_reference(&self) -> Option<&str> {
        self.attribute("xmlns")
    }
}

fn first_value<'a>(values: &'a [V8ItemValue], name: &str) -> Option<&'a str> {
    values
        .iter()
        .find(|item| &*item.name == name)
        .map(|item| &*item.value)
}

/// The item instances of one call, and what the limit left raw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct V8ItemInstances {
    /// The decoded instances in stored order.
    pub instances: Vec<V8ItemInstance>,
    /// Item entries that were not decoded because their data was larger
    /// than what the call had left of [`V8_MAX_ITEM_BYTES`]. They stay raw
    /// in their records.
    pub over_limit: usize,
}

/// Decode the item instances among `entries`, in stored order.
///
/// An entry of another type, and an item entry that does not match the
/// observed layout, is not listed and costs nothing. One call returns at
/// most [`V8_MAX_ITEM_BYTES`] stored bytes as instances: the data of each
/// returned instance counts, and an item entry whose data is larger than
/// what is left is counted in `over_limit` and not read, also when it is
/// damaged. The entries after it are still read. This is not an error.
#[must_use]
pub fn decode_v8_item_instances(
    entries: impl IntoIterator<Item = V8AuxiliaryEntry>,
) -> V8ItemInstances {
    let mut left = V8_MAX_ITEM_BYTES;
    let mut result = V8ItemInstances::default();
    for entry in entries {
        if entry.type_id != V8_ITEM_INSTANCE_ENTRY_TYPE {
            continue;
        }
        let data = entry.data();
        if data.len() > left {
            result.over_limit += 1;
        } else if let Some(item) = decode_layout(entry.entry_id, data) {
            left -= data.len();
            result.instances.push(item);
        }
    }
    result
}

/// Decode an auxiliary entry of type [`V8_ITEM_INSTANCE_ENTRY_TYPE`].
///
/// Returns `None` for another entry type, for data that does not match the
/// observed layout exactly, and for data of more than [`V8_MAX_ITEM_BYTES`]
/// bytes. The entry keeps its raw bytes in every case.
#[must_use]
pub fn decode_v8_item_instance(entry: &V8AuxiliaryEntry) -> Option<V8ItemInstance> {
    if entry.type_id != V8_ITEM_INSTANCE_ENTRY_TYPE {
        return None;
    }
    decode_v8_item_data(entry.entry_id, entry.data())
}

/// Decode the data block of an item-instance entry.
///
/// The decoder accepts only the observed layout: the fixed head and
/// signature, a string table of at most 127 strings of at most 127 units,
/// and the token sequence root, attributes, content mark, properties, end.
/// Every string number must exist and every byte must be consumed. Data of
/// more than [`V8_MAX_ITEM_BYTES`] bytes is not decoded.
#[must_use]
pub fn decode_v8_item_data(entry_id: u32, data: &[u8]) -> Option<V8ItemInstance> {
    if data.len() > V8_MAX_ITEM_BYTES {
        return None;
    }
    decode_layout(entry_id, data)
}

fn decode_layout(entry_id: u32, data: &[u8]) -> Option<V8ItemInstance> {
    if data.get(..HEAD.len())? != HEAD {
        return None;
    }
    let schema_element_id = read_u32(data, SCHEMA_ID_OFFSET)?;
    if data.get(SIGNATURE_OFFSET..STRING_COUNT_OFFSET)? != SIGNATURE {
        return None;
    }
    let mut cursor = Cursor {
        data,
        offset: STRING_COUNT_OFFSET,
    };
    // The string table is decoded once. Every reference below is a handle
    // to one of these strings.
    let count = cursor.small()?;
    let mut strings = Vec::with_capacity(count);
    for _ in 0..count {
        strings.push(cursor.string()?);
    }

    cursor.expect(TOKEN_ROOT)?;
    let class_name = cursor.reference(&strings)?;
    let mut attributes = Vec::with_capacity(cursor.run(TOKEN_ATTRIBUTE, ATTRIBUTE_BYTES));
    while cursor.peek()? == TOKEN_ATTRIBUTE {
        cursor.offset += 1;
        let name = cursor.reference(&strings)?;
        cursor.expect(TOKEN_VALUE)?;
        let value = cursor.reference(&strings)?;
        attributes.push(V8ItemValue { name, value });
    }
    if attributes.is_empty() {
        return None;
    }
    cursor.expect(TOKEN_CONTENT)?;
    let mut properties = Vec::with_capacity(cursor.run(TOKEN_CHILD, PROPERTY_BYTES));
    while cursor.peek()? == TOKEN_CHILD {
        cursor.offset += 1;
        let name = cursor.reference(&strings)?;
        cursor.expect(TOKEN_VALUE)?;
        let value = cursor.reference(&strings)?;
        cursor.expect(TOKEN_END)?;
        properties.push(V8ItemValue { name, value });
    }
    cursor.expect(TOKEN_END)?;
    (cursor.offset == data.len()).then_some(V8ItemInstance {
        entry_id,
        schema_element_id,
        class_name,
        attributes,
        properties,
    })
}

struct Cursor<'a> {
    data: &'a [u8],
    offset: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.data.get(self.offset).copied()
    }

    fn byte(&mut self) -> Option<u8> {
        let value = self.peek()?;
        self.offset += 1;
        Some(value)
    }

    fn expect(&mut self, token: u8) -> Option<()> {
        (self.byte()? == token).then_some(())
    }

    /// A count, length or string number stored in one byte.
    fn small(&mut self) -> Option<usize> {
        let value = self.byte()?;
        (value < SINGLE_BYTE_LIMIT).then_some(usize::from(value))
    }

    fn take(&mut self, length: usize) -> Option<&[u8]> {
        let end = self.offset.checked_add(length)?;
        let bytes = self.data.get(self.offset..end)?;
        self.offset = end;
        Some(bytes)
    }

    /// How many groups of `width` bytes that start with `token` come next.
    /// A list gets its room from this count, so it never holds more room
    /// than its stored bytes ask for.
    fn run(&self, token: u8, width: usize) -> usize {
        self.data
            .get(self.offset..)
            .unwrap_or_default()
            .chunks_exact(width)
            .take_while(|group| group[0] == token)
            .count()
    }

    fn string(&mut self) -> Option<Arc<str>> {
        match self.byte()? {
            // One byte for each character: the byte value is the code point.
            STRING_ONE_BYTE => {
                let length = self.small()?;
                let text = self.take(length)?.iter().copied().map(char::from);
                Some(text.collect::<String>().into())
            }
            STRING_UTF16 => {
                let units = self.small()?;
                let bytes = self.take(units.checked_mul(2)?)?;
                let units = bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect::<Vec<_>>();
                String::from_utf16(&units).ok().map(Arc::from)
            }
            _ => None,
        }
    }

    /// A handle to the string a string number names. No text is copied.
    fn reference(&mut self, strings: &[Arc<str>]) -> Option<Arc<str>> {
        strings.get(self.small()?).cloned()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A string table entry in the one-byte or the UTF-16 form.
    pub(crate) fn table_string(text: &str) -> Vec<u8> {
        if text.chars().all(|character| u32::from(character) < 0x100) {
            let mut bytes = vec![STRING_ONE_BYTE, u8::try_from(text.chars().count()).unwrap()];
            bytes.extend(text.chars().map(|character| character as u8));
            bytes
        } else {
            let units = text.encode_utf16().collect::<Vec<_>>();
            let mut bytes = vec![STRING_UTF16, u8::try_from(units.len()).unwrap()];
            bytes.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
            bytes
        }
    }

    /// Head, schema ID, signature, string table and the given token bytes.
    pub(crate) fn item_bytes(schema_element_id: u32, strings: &[&str], tokens: &[u8]) -> Vec<u8> {
        let mut bytes = HEAD.to_vec();
        bytes.extend(schema_element_id.to_le_bytes());
        bytes.extend(SIGNATURE);
        bytes.push(u8::try_from(strings.len()).unwrap());
        for text in strings {
            bytes.extend(table_string(text));
        }
        bytes.extend(tokens);
        bytes
    }

    fn number<'a>(text: &'a str, strings: &mut Vec<&'a str>) -> u8 {
        let position = strings
            .iter()
            .position(|known| *known == text)
            .unwrap_or_else(|| {
                strings.push(text);
                strings.len() - 1
            });
        u8::try_from(position).unwrap()
    }

    /// A complete instance. Equal strings share one table entry, numbered in
    /// the order of first use.
    pub(crate) fn item_data<'a>(
        schema_element_id: u32,
        class_name: &'a str,
        attributes: &[(&'a str, &'a str)],
        properties: &[(&'a str, &'a str)],
    ) -> Vec<u8> {
        let mut strings = Vec::new();
        let mut tokens = vec![TOKEN_ROOT, number(class_name, &mut strings)];
        for (name, value) in attributes {
            tokens.extend([TOKEN_ATTRIBUTE, number(name, &mut strings)]);
            tokens.extend([TOKEN_VALUE, number(value, &mut strings)]);
        }
        tokens.push(TOKEN_CONTENT);
        for (name, value) in properties {
            tokens.extend([TOKEN_CHILD, number(name, &mut strings)]);
            tokens.extend([TOKEN_VALUE, number(value, &mut strings), TOKEN_END]);
        }
        tokens.push(TOKEN_END);
        item_bytes(schema_element_id, &strings, &tokens)
    }

    /// The bytes an instance asks for: its own fixed part, every string
    /// once (its text and the two counters of its shared block) and the
    /// room of the two lists.
    pub(crate) fn memory(item: &V8ItemInstance) -> usize {
        let values = item.attributes.iter().chain(&item.properties);
        let mut strings = std::iter::once(&item.class_name)
            .chain(values.flat_map(|value| [&value.name, &value.value]))
            .map(|text| (Arc::as_ptr(text).cast::<u8>(), text.len()))
            .collect::<Vec<_>>();
        strings.sort_unstable();
        strings.dedup_by_key(|(address, _)| *address);
        let text = strings
            .iter()
            .map(|(_, length)| length + 2 * size_of::<usize>())
            .sum::<usize>();
        let lists =
            (item.attributes.capacity() + item.properties.capacity()) * size_of::<V8ItemValue>();
        size_of::<V8ItemInstance>() + text + lists
    }

    /// Most bytes of memory for one stored byte: 8 on a 64-bit target. An
    /// attribute is 4 stored bytes and two handles. An empty table string
    /// is 2 stored bytes and the two counters of its block. The fixed part
    /// of an instance is paid by the 30 stored bytes that every instance
    /// has besides its strings and its uses of them. These are requested
    /// bytes, as [`memory`] counts them: the overhead of the allocator is
    /// not in the number, and it can add up to about 2 times for small
    /// blocks.
    pub(crate) const MEMORY_PER_STORED_BYTE: usize = size_of::<usize>();
}

#[cfg(test)]
mod tests {
    use super::test_support::{item_bytes, item_data, memory, MEMORY_PER_STORED_BYTE};
    use super::*;
    use crate::v8::auxiliary::decode_v8_auxiliary_entries;
    use crate::v8::auxiliary::test_support::{entry, payload};

    const ATTRIBUTES: [(&str, &str); 2] = [
        ("instanceId", ""),
        ("xmlns", "Sample__x0020__Library.01.00"),
    ];

    fn pairs(values: &[V8ItemValue]) -> Vec<(&str, &str)> {
        values
            .iter()
            .map(|item| (&*item.name, &*item.value))
            .collect()
    }

    #[test]
    fn decodes_class_attributes_and_properties_in_stored_order() {
        let data = item_data(
            0x0001_02b3,
            "Pump",
            &ATTRIBUTES,
            &[
                ("Tag", "P-101"),
                ("Duty", ""),
                ("Standby", "P-101"),
                ("Note", ""),
            ],
        );
        let item = decode_v8_item_data(2, &data).unwrap();
        assert_eq!(item.entry_id, 2);
        assert_eq!(item.schema_element_id, 0x0001_02b3);
        assert_eq!(&*item.class_name, "Pump");
        assert_eq!(pairs(&item.attributes), ATTRIBUTES);
        assert_eq!(
            pairs(&item.properties),
            [
                ("Tag", "P-101"),
                ("Duty", ""),
                ("Standby", "P-101"),
                ("Note", "")
            ]
        );
        assert_eq!(
            item.schema_reference(),
            Some("Sample__x0020__Library.01.00")
        );
        assert_eq!(item.attribute("instanceId"), Some(""));
        assert_eq!(item.property("Standby"), Some("P-101"));
        assert_eq!(item.property("Missing"), None);
    }

    #[test]
    fn the_byte_layout_is_the_documented_one() {
        // Written out by hand: the string table shares the empty string and
        // the repeated value, and numbers the strings in the order of use.
        let mut expected = vec![0x00, 0x00, 0x01, 0x01, 0x15, 0x2a, 0x00, 0x00, 0x00, 0x01];
        expected.extend(b"BECXML\x00\xff\x0a\x0d\x01\x00\x00\x03\x00\x00\x30");
        expected.push(7);
        for text in [
            "Pump",
            "instanceId",
            "",
            "xmlns",
            "Lib.01.00",
            "Tag",
            "P-101",
        ] {
            expected.extend([0xfa, u8::try_from(text.len()).unwrap()]);
            expected.extend(text.as_bytes());
        }
        expected.extend([0x03, 0, 0x05, 1, 0x11, 2, 0x05, 3, 0x11, 4, 0x06]);
        expected.extend([0x02, 5, 0x11, 6, 0x04, 0x04]);
        let data = item_data(
            42,
            "Pump",
            &[("instanceId", ""), ("xmlns", "Lib.01.00")],
            &[("Tag", "P-101")],
        );
        assert_eq!(data, expected);
        assert_eq!(decode_v8_item_data(1, &data).unwrap().schema_element_id, 42);
    }

    #[test]
    fn one_byte_strings_are_code_points_and_wide_strings_are_utf16() {
        let data = item_data(
            1,
            "Pipe",
            &ATTRIBUTES,
            &[
                ("Bore", "1\u{bd}\""),
                ("Old", "\u{81}"),
                ("Wide", "2\u{192}\" \u{1f600}"),
            ],
        );
        // the one-byte form stores U+00BD and U+0081 as the bytes bd and 81
        assert!(data
            .windows(5)
            .any(|window| window == [0xfa, 3, b'1', 0xbd, b'"']));
        assert!(data.windows(3).any(|window| window == [0xfa, 1, 0x81]));
        // the wide form counts UTF-16 units: six for these five characters
        assert!(data
            .windows(6)
            .any(|window| window == [0xfb, 6, b'2', 0x00, 0x92, 0x01]));
        let item = decode_v8_item_data(1, &data).unwrap();
        assert_eq!(item.property("Bore"), Some("1\u{bd}\""));
        assert_eq!(item.property("Old"), Some("\u{81}"));

        // A one-byte string is not a code page: byte 83 is U+0083, and U+0192
        // (byte 83 in Windows-1252) needs the wide form.
        let narrow = item_bytes(
            1,
            &["A", "\u{83}"],
            &[0x03, 0, 0x05, 0, 0x11, 1, 0x06, 0x04],
        );
        assert!(narrow.windows(3).any(|window| window == [0xfa, 1, 0x83]));
        let narrow_item = decode_v8_item_data(1, &narrow).unwrap();
        assert_eq!(narrow_item.attribute("A"), Some("\u{83}"));
        assert_eq!(item.property("Wide"), Some("2\u{192}\" \u{1f600}"));
    }

    #[test]
    fn an_instance_without_properties_is_valid() {
        let data = item_data(1, "Empty", &ATTRIBUTES, &[]);
        let item = decode_v8_item_data(1, &data).unwrap();
        assert_eq!(&*item.class_name, "Empty");
        assert!(item.properties.is_empty());
    }

    #[test]
    fn only_item_entries_are_decoded() {
        let data = item_data(1, "Pump", &ATTRIBUTES, &[("Tag", "P-101")]);
        let bytes = payload(&[
            entry(0x5709_0000, 1, false, &data),
            entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 3, true, &data),
            entry(V8_ITEM_INSTANCE_ENTRY_TYPE | 1, 4, true, &data),
        ]);
        let entries = decode_v8_auxiliary_entries(&bytes, 3).unwrap();
        assert!(decode_v8_item_instance(&entries[0]).is_none());
        assert_eq!(decode_v8_item_instance(&entries[1]).unwrap().entry_id, 3);
        assert!(decode_v8_item_instance(&entries[2]).is_none());
    }

    #[test]
    fn layouts_that_were_not_observed_stay_raw() {
        let strings = ["Pump", "instanceId", "", "Tag", "P-101", "Part"];
        let good = [
            0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x02, 3, 0x11, 4, 0x04, 0x04,
        ];
        assert!(decode_v8_item_data(1, &item_bytes(1, &strings, &good)).is_some());
        for (case, tokens) in [
            ("no root", &[0x02, 0, 0x05, 1, 0x11, 2, 0x06, 0x04][..]),
            (
                "no attribute",
                &[0x03, 0, 0x06, 0x02, 3, 0x11, 4, 0x04, 0x04],
            ),
            ("attribute without value", &[0x03, 0, 0x05, 1, 0x06, 0x04]),
            (
                "no content mark",
                &[0x03, 0, 0x05, 1, 0x11, 2, 0x02, 3, 0x11, 4, 0x04, 0x04],
            ),
            (
                "property without value",
                &[0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x02, 3, 0x04, 0x04],
            ),
            (
                "nested element",
                &[
                    0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x02, 3, 0x02, 5, 0x11, 4, 0x04, 0x04, 0x04,
                ],
            ),
            (
                "attribute on a property",
                &[
                    0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x02, 3, 0x05, 1, 0x11, 2, 0x11, 4, 0x04, 0x04,
                ],
            ),
            (
                "two values",
                &[
                    0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x02, 3, 0x11, 4, 0x11, 4, 0x04, 0x04,
                ],
            ),
            (
                "root not closed",
                &[0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x02, 3, 0x11, 4, 0x04],
            ),
            (
                "bytes after the end",
                &[
                    0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x02, 3, 0x11, 4, 0x04, 0x04, 0x00,
                ],
            ),
            (
                "second root",
                &[
                    0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x04, 0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x04,
                ],
            ),
            (
                "unknown token",
                &[
                    0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x07, 3, 0x11, 4, 0x04, 0x04,
                ],
            ),
            (
                "string number past the table",
                &[
                    0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x02, 6, 0x11, 4, 0x04, 0x04,
                ],
            ),
            (
                "string number with the high bit",
                &[
                    0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x02, 0x83, 0x11, 4, 0x04, 0x04,
                ],
            ),
            ("no tokens", &[]),
        ] {
            let data = item_bytes(1, &strings, tokens);
            assert!(decode_v8_item_data(1, &data).is_none(), "{case}");
        }
    }

    #[test]
    fn head_signature_and_string_table_must_match_exactly() {
        let good = item_data(1, "Pump", &ATTRIBUTES, &[("Tag", "P-101")]);
        assert!(decode_v8_item_data(1, &good).is_some());
        // every byte of the head and of the signature is required
        for offset in (0..HEAD.len()).chain(SIGNATURE_OFFSET..STRING_COUNT_OFFSET) {
            let mut changed = good.clone();
            changed[offset] ^= 0x01;
            assert!(
                decode_v8_item_data(1, &changed).is_none(),
                "offset {offset}"
            );
        }
        // the schema ID is data, not a constant
        let mut other_schema = good.clone();
        other_schema[SCHEMA_ID_OFFSET] ^= 0x01;
        assert!(decode_v8_item_data(1, &other_schema).is_some());

        // a string count that does not fit one observed byte
        let mut count = good.clone();
        count[STRING_COUNT_OFFSET] = 0x80;
        assert!(decode_v8_item_data(1, &count).is_none());
        // a string tag other than the two observed ones
        let mut tag = good.clone();
        tag[STRING_COUNT_OFFSET + 1] = 0xfc;
        assert!(decode_v8_item_data(1, &tag).is_none());
        // a string length with the high bit, with enough bytes to cover it
        let mut long = HEAD.to_vec();
        long.extend(1_u32.to_le_bytes());
        long.extend(SIGNATURE);
        long.extend([1, STRING_ONE_BYTE, 0x80]);
        long.extend([b'a'; 0x80]);
        long.extend([
            TOKEN_ROOT,
            0,
            TOKEN_ATTRIBUTE,
            0,
            TOKEN_VALUE,
            0,
            TOKEN_CONTENT,
            TOKEN_END,
        ]);
        assert!(decode_v8_item_data(1, &long).is_none());
        long[STRING_COUNT_OFFSET + 2] = 0x7f;
        long.remove(STRING_COUNT_OFFSET + 3);
        assert!(decode_v8_item_data(1, &long).is_some());
        // a wide string that is not valid UTF-16 (a lone surrogate)
        let mut surrogate = HEAD.to_vec();
        surrogate.extend(1_u32.to_le_bytes());
        surrogate.extend(SIGNATURE);
        surrogate.extend([1, STRING_UTF16, 1, 0x00, 0xd8]);
        surrogate.extend([
            TOKEN_ROOT,
            0,
            TOKEN_ATTRIBUTE,
            0,
            TOKEN_VALUE,
            0,
            TOKEN_CONTENT,
            TOKEN_END,
        ]);
        assert!(decode_v8_item_data(1, &surrogate).is_none());
        surrogate[STRING_COUNT_OFFSET + 4] = 0x00;
        assert!(decode_v8_item_data(1, &surrogate).is_some());
    }

    /// The longest string the table can hold: 127 UTF-16 units that each
    /// take three bytes of UTF-8.
    const LONG_BYTES: usize = 127 * 3;

    /// An instance whose table holds the one long string, used as the class
    /// name and as the name and the value of `pairs` attributes. Each pair is
    /// four stored bytes that name 762 bytes of text.
    fn amplifying(pairs: usize) -> Vec<u8> {
        let long = "\u{3042}".repeat(127);
        assert_eq!(long.len(), LONG_BYTES);
        let mut tokens = vec![TOKEN_ROOT, 0];
        for _ in 0..pairs {
            tokens.extend([TOKEN_ATTRIBUTE, 0, TOKEN_VALUE, 0]);
        }
        tokens.extend([TOKEN_CONTENT, TOKEN_END]);
        item_bytes(1, &[&long], &tokens)
    }

    /// Stored bytes of [`amplifying`] without its pairs.
    const AMPLIFYING_FIXED_BYTES: usize = STRING_COUNT_OFFSET + 1 + 2 + 127 * 2 + 4;

    #[test]
    fn a_table_string_is_decoded_once_and_shared_by_every_use() {
        let pairs = 1_000;
        let item = decode_v8_item_data(1, &amplifying(pairs)).unwrap();
        assert_eq!(item.attributes.len(), pairs);
        assert_eq!(item.class_name.len(), LONG_BYTES);
        for pair in &item.attributes {
            assert!(Arc::ptr_eq(&pair.name, &item.class_name));
            assert!(Arc::ptr_eq(&pair.value, &item.class_name));
        }
        // one block of text, and nothing else holds it after the decode
        assert_eq!(Arc::strong_count(&item.class_name), 1 + 2 * pairs);
        // a clone of the instance copies handles, not text
        let copy = item.clone();
        assert!(Arc::ptr_eq(&copy.attributes[0].value, &item.class_name));

        // properties share in the same way, and two table entries with
        // equal text stay two strings
        let tokens = [
            0x03, 0, 0x05, 1, 0x11, 2, 0x06, 0x02, 1, 0x11, 2, 0x04, 0x02, 3, 0x11, 2, 0x04, 0x04,
        ];
        let item = decode_v8_item_data(1, &item_bytes(1, &["A", "B", "C", "B"], &tokens)).unwrap();
        assert!(Arc::ptr_eq(
            &item.attributes[0].name,
            &item.properties[0].name
        ));
        assert!(Arc::ptr_eq(
            &item.attributes[0].value,
            &item.properties[1].value
        ));
        assert_eq!(item.properties[0].name, item.properties[1].name);
        assert!(!Arc::ptr_eq(
            &item.properties[0].name,
            &item.properties[1].name
        ));
        // a table string that nothing names is gone with the table
        assert_eq!(Arc::strong_count(&item.class_name), 1);
    }

    #[test]
    fn the_decoded_size_is_a_fixed_multiple_of_the_stored_size() {
        let empty = |attributes: usize, properties: usize| {
            let mut tokens = vec![TOKEN_ROOT, 0];
            for _ in 0..attributes {
                tokens.extend([TOKEN_ATTRIBUTE, 0, TOKEN_VALUE, 0]);
            }
            tokens.push(TOKEN_CONTENT);
            for _ in 0..properties {
                tokens.extend([TOKEN_CHILD, 0, TOKEN_VALUE, 0, TOKEN_END]);
            }
            tokens.push(TOKEN_END);
            item_bytes(1, &[""], &tokens)
        };
        // 127 strings that are each used once: empty, short, and the two
        // long forms
        let table = |text: &str| {
            let strings = vec![text; 127];
            let mut tokens = vec![TOKEN_ROOT, 0];
            for number in (1..127).step_by(2) {
                tokens.extend([TOKEN_ATTRIBUTE, number, TOKEN_VALUE, number + 1]);
            }
            tokens.extend([TOKEN_CONTENT, TOKEN_END]);
            item_bytes(1, &strings, &tokens)
        };
        let cases = [
            ("smallest instance", empty(1, 0)),
            ("empty strings, attributes", empty(50_000, 0)),
            ("empty strings, properties", empty(1, 50_000)),
            ("one long string, few uses", amplifying(1)),
            ("one long string, many uses", amplifying(50_000)),
            ("a table of empty strings", table("")),
            ("a table of short strings", table("ab")),
            (
                "a table of long one-byte strings",
                table(&"\u{ff}".repeat(127)),
            ),
            (
                "a table of long wide strings",
                table(&"\u{3042}".repeat(127)),
            ),
            (
                "an ordinary instance",
                item_data(1, "Pump", &ATTRIBUTES, &[("Tag", "P-101"), ("Note", "")]),
            ),
        ];
        for (case, data) in cases {
            let item = decode_v8_item_data(1, &data).unwrap();
            assert_eq!(item.attributes.capacity(), item.attributes.len(), "{case}");
            assert_eq!(item.properties.capacity(), item.properties.len(), "{case}");
            assert!(
                memory(&item) <= MEMORY_PER_STORED_BYTE * data.len(),
                "{case}: {} bytes of memory for {} stored bytes",
                memory(&item),
                data.len()
            );
        }
        // The smallest instance is 38 stored bytes: 30 fixed bytes, one
        // string, the class name and one attribute. The 30 bytes pay for
        // the fixed part, also with the spare room of a list of instances.
        assert_eq!(empty(1, 0).len(), 38);
        assert!(2 * size_of::<V8ItemInstance>() <= MEMORY_PER_STORED_BYTE * 30);

        // The text that the many uses name is far larger than the memory.
        let data = amplifying(50_000);
        let item = decode_v8_item_data(1, &data).unwrap();
        let named = item
            .attributes
            .iter()
            .map(|pair| pair.name.len() + pair.value.len())
            .sum::<usize>();
        assert_eq!(named, 50_000 * 2 * LONG_BYTES);
        assert!(named > 150 * data.len());
        assert!(memory(&item) < named / 20);
    }

    #[test]
    fn one_instance_is_held_to_the_stored_byte_limit() {
        // exactly the limit: decoded
        let pairs = (V8_MAX_ITEM_BYTES - AMPLIFYING_FIXED_BYTES) / ATTRIBUTE_BYTES;
        let at_limit = amplifying(pairs);
        assert_eq!(at_limit.len(), V8_MAX_ITEM_BYTES);
        let item = decode_v8_item_data(1, &at_limit).unwrap();
        assert_eq!(item.attributes.len(), pairs);
        // one more pair is four more stored bytes: not decoded
        let over = amplifying(pairs + 1);
        assert_eq!(over.len(), V8_MAX_ITEM_BYTES + ATTRIBUTE_BYTES);
        assert!(decode_v8_item_data(1, &over).is_none());
        let bytes = payload(&[entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, true, &over)]);
        let entries = decode_v8_auxiliary_entries(&bytes, 1).unwrap();
        assert!(decode_v8_item_instance(&entries[0]).is_none());
    }

    fn decode_entries(blobs: &[(u32, &[u8])]) -> V8ItemInstances {
        let entries = blobs
            .iter()
            .zip(1..)
            .map(|((type_id, data), entry_id)| entry(*type_id, entry_id, true, data))
            .collect::<Vec<_>>();
        let bytes = payload(&entries);
        let count = u32::try_from(entries.len()).unwrap();
        decode_v8_item_instances(decode_v8_auxiliary_entries(&bytes, count).unwrap())
    }

    fn entry_ids(decoded: &V8ItemInstances) -> Vec<u32> {
        decoded.instances.iter().map(|item| item.entry_id).collect()
    }

    #[test]
    fn one_call_for_many_entries_is_held_to_the_stored_byte_limit() {
        // each large instance takes more than half of the limit
        let large = amplifying(V8_MAX_ITEM_BYTES / (2 * ATTRIBUTE_BYTES) + 1);
        assert!(large.len() > V8_MAX_ITEM_BYTES / 2 && large.len() < V8_MAX_ITEM_BYTES);
        let small = item_data(1, "Pump", &ATTRIBUTES, &[("Tag", "P-101")]);
        let item = V8_ITEM_INSTANCE_ENTRY_TYPE;

        // Each of the two large instances decodes alone. Together they pass
        // the limit, so the second stays raw and is counted. A small entry
        // after it still fits and is decoded.
        assert!(decode_v8_item_data(1, &large).is_some());
        let decoded = decode_entries(&[
            (item, &large),
            (0x5709_0000, &small),
            (item, &large),
            (item, &small),
            (0x5709_0000, &large),
            (item, &large),
        ]);
        assert_eq!(entry_ids(&decoded), [1, 4]);
        assert_eq!(decoded.over_limit, 2);
        let stored = large.len() + small.len();
        assert!(stored <= V8_MAX_ITEM_BYTES);
        let total = decoded.instances.iter().map(memory).sum::<usize>();
        assert!(total <= MEMORY_PER_STORED_BYTE * stored);

        // Without the limit in reach nothing is counted.
        let decoded = decode_entries(&[(item, &small), (item, &small)]);
        assert_eq!(entry_ids(&decoded), [1, 2]);
        assert_eq!(decoded.over_limit, 0);
    }

    #[test]
    fn a_damaged_entry_stays_raw_and_hides_no_entry_after_it() {
        let small = item_data(1, "Pump", &ATTRIBUTES, &[("Tag", "P-101")]);
        let item = V8_ITEM_INSTANCE_ENTRY_TYPE;
        // Large entries whose layout fails only at their end: a missing
        // last token, a string number past the table, a byte after the end.
        let large = amplifying(V8_MAX_ITEM_BYTES / (2 * ATTRIBUTE_BYTES) + 1);
        let mut cut = large.clone();
        cut.pop();
        let mut number = large.clone();
        let last_number = number.len() - 3;
        number[last_number] = 1;
        let mut longer = large.clone();
        longer.push(0);
        for damaged in [&cut, &number, &longer] {
            assert!(decode_v8_item_data(1, damaged).is_none());
        }

        // None of them is charged or counted, and every valid entry after
        // them is decoded: also the large one, which needs most of the limit.
        let decoded = decode_entries(&[
            (item, &cut),
            (item, &small),
            (item, &number),
            (item, &longer),
            (item, &large),
            (item, &small),
        ]);
        assert_eq!(entry_ids(&decoded), [2, 5, 6]);
        assert_eq!(decoded.over_limit, 0);

        let mut damaged = small.clone();
        damaged.pop();
        let decoded = decode_entries(&[(item, &small), (item, &damaged), (item, &small)]);
        assert_eq!(entry_ids(&decoded), [1, 3]);
        assert_eq!(decoded.over_limit, 0);
    }

    #[test]
    fn an_entry_larger_than_what_is_left_is_counted_also_when_it_is_damaged() {
        // The stored size alone decides. The call does not read an entry
        // that is larger than what it has left, so it cannot know that the
        // entry is damaged: it counts it.
        let large = amplifying(V8_MAX_ITEM_BYTES / (2 * ATTRIBUTE_BYTES) + 1);
        let mut cut = large.clone();
        cut.pop();
        assert!(decode_v8_item_data(1, &cut).is_none());
        let small = item_data(1, "Pump", &ATTRIBUTES, &[("Tag", "P-101")]);
        let item = V8_ITEM_INSTANCE_ENTRY_TYPE;
        let decoded = decode_entries(&[(item, &large), (item, &cut), (item, &small)]);
        assert_eq!(entry_ids(&decoded), [1, 3]);
        assert_eq!(decoded.over_limit, 1);

        // The same damaged entry first: it fits, it is read, it fails, and
        // it is neither charged nor counted.
        let decoded = decode_entries(&[(item, &cut), (item, &large), (item, &small)]);
        assert_eq!(entry_ids(&decoded), [2, 3]);
        assert_eq!(decoded.over_limit, 0);

        // A damaged entry that is larger than the whole limit is counted
        // in a call that has decoded nothing yet.
        let mut over =
            amplifying((V8_MAX_ITEM_BYTES - AMPLIFYING_FIXED_BYTES) / ATTRIBUTE_BYTES + 1);
        over.pop();
        assert!(over.len() > V8_MAX_ITEM_BYTES);
        let decoded = decode_entries(&[(item, &over), (item, &small)]);
        assert_eq!(entry_ids(&decoded), [2]);
        assert_eq!(decoded.over_limit, 1);
    }

    #[test]
    fn every_prefix_and_single_byte_mutation_is_panic_free() {
        let good = item_data(
            7,
            "Pump",
            &ATTRIBUTES,
            &[("Tag", "P-101"), ("Wide", "\u{192}"), ("Note", "")],
        );
        let reference = decode_v8_item_data(1, &good).unwrap();
        for end in 0..good.len() {
            assert!(
                decode_v8_item_data(1, &good[..end]).is_none(),
                "prefix {end}"
            );
        }
        for offset in 0..good.len() {
            for value in 0..=u8::MAX {
                let mut mutated = good.clone();
                mutated[offset] = value;
                if let Some(item) = decode_v8_item_data(1, &mutated) {
                    // a mutation that still decodes changed text or the schema ID only
                    assert_eq!(item.properties.len(), reference.properties.len());
                    assert_eq!(item.attributes.len(), reference.attributes.len());
                }
            }
        }
    }
}
