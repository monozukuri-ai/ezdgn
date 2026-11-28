use std::ops::Range;
use std::sync::Arc;

use super::raw::{read_u32, V8AuxiliaryRecord};

const ENTRY_HEADER_BYTES: usize = 16;
const PAYLOAD_TRAILER: [u8; 4] = [0x01, 0x00, 0x00, 0x00];
const ENTRY_SIZE_MASK: u32 = 0x7fff_ffff;

/// Most entries one auxiliary payload is split into.
///
/// The entry count comes from the file and an entry can be an empty
/// 16-byte header, so a large record could ask for a long list of small
/// entries. A payload that declares more entries is not split and stays
/// raw. The largest count seen in one record is 4 in the public files and
/// 8 in the confidential files.
pub const V8_MAX_AUXILIARY_ENTRIES: usize = 1 << 16;

/// One framed entry of an auxiliary-record payload.
///
/// An auxiliary record holds every entry attached to one element. The header
/// word that the raw record exposes as `flags` is the entry count. Each entry
/// has a 16-byte header and a data block; the meaning of the data depends on
/// `type_id` and stays raw here.
///
/// Two entries are equal when their position, their header words and their
/// data are equal. The page bytes they are cut from are not compared.
#[derive(Debug, Clone)]
pub struct V8AuxiliaryEntry {
    /// Position of the entry within its record.
    pub index: usize,
    /// Byte offset of the entry header within the record payload.
    pub offset: usize,
    /// Type word at `+0x00`. The high half groups related entry types and the
    /// low half numbers the types of one group.
    pub type_id: u32,
    /// Entry identifier at `+0x04`; it tells entries of one type apart.
    pub entry_id: u32,
    /// Size word at `+0x08` exactly as stored. The low 31 bits are the data
    /// length; bit 31 is a writer flag whose meaning is unresolved.
    pub size_word: u32,
    /// Word at `+0x0c`, zero in every observed entry.
    pub reserved: u32,
    bytes: Arc<[u8]>,
    data_range: Range<usize>,
}

impl PartialEq for V8AuxiliaryEntry {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
            && self.offset == other.offset
            && self.type_id == other.type_id
            && self.entry_id == other.entry_id
            && self.size_word == other.size_word
            && self.reserved == other.reserved
            && self.data() == other.data()
    }
}

impl Eq for V8AuxiliaryEntry {}

impl V8AuxiliaryEntry {
    /// High half of [`Self::type_id`].
    #[must_use]
    pub const fn type_major(&self) -> u16 {
        (self.type_id >> 16) as u16
    }

    /// Low half of [`Self::type_id`].
    #[must_use]
    pub const fn type_minor(&self) -> u16 {
        self.type_id as u16
    }

    /// Bit 31 of the stored size word.
    #[must_use]
    pub const fn size_flag(&self) -> bool {
        self.size_word & !ENTRY_SIZE_MASK != 0
    }

    /// The exact data bytes of the entry.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.bytes[self.data_range.clone()]
    }
}

impl V8AuxiliaryRecord {
    /// Split the payload into its entries.
    ///
    /// Returns `None` when the payload does not follow the entry layout
    /// exactly or declares more than [`V8_MAX_AUXILIARY_ENTRIES`] entries;
    /// the record then remains available only as raw bytes.
    #[must_use]
    pub fn entries(&self) -> Option<Vec<V8AuxiliaryEntry>> {
        let (bytes, payload_range) = self.shared_payload();
        split_entries(bytes, payload_range, self.flags)
    }
}

/// Split one auxiliary payload that declares `count` entries.
///
/// The payload must hold exactly `count` entries, each a 16-byte header
/// followed by its declared data, and then the four bytes `01 00 00 00`.
/// Any other shape returns `None`; nothing is guessed or skipped. A count
/// above [`V8_MAX_AUXILIARY_ENTRIES`] also returns `None`.
#[must_use]
pub fn decode_v8_auxiliary_entries(payload: &[u8], count: u32) -> Option<Vec<V8AuxiliaryEntry>> {
    split_entries(Arc::from(payload), 0..payload.len(), count)
}

fn split_entries(
    bytes: Arc<[u8]>,
    payload_range: Range<usize>,
    count: u32,
) -> Option<Vec<V8AuxiliaryEntry>> {
    let payload = bytes.get(payload_range.clone())?;
    let count = usize::try_from(count).ok()?;
    if count > V8_MAX_AUXILIARY_ENTRIES {
        return None;
    }
    let minimum = count
        .checked_mul(ENTRY_HEADER_BYTES)?
        .checked_add(PAYLOAD_TRAILER.len())?;
    if payload.len() < minimum {
        return None;
    }
    let mut entries = Vec::with_capacity(count);
    let mut offset = 0usize;
    for index in 0..count {
        let header_end = offset.checked_add(ENTRY_HEADER_BYTES)?;
        let header = payload.get(offset..header_end)?;
        let type_id = read_u32(header, 0)?;
        let entry_id = read_u32(header, 4)?;
        let size_word = read_u32(header, 8)?;
        let reserved = read_u32(header, 12)?;
        let size = usize::try_from(size_word & ENTRY_SIZE_MASK).ok()?;
        let end = header_end.checked_add(size)?;
        payload.get(header_end..end)?;
        entries.push(V8AuxiliaryEntry {
            index,
            offset,
            type_id,
            entry_id,
            size_word,
            reserved,
            bytes: Arc::clone(&bytes),
            data_range: payload_range.start + header_end..payload_range.start + end,
        });
        offset = end;
    }
    (payload.get(offset..)? == PAYLOAD_TRAILER).then_some(entries)
}

#[cfg(test)]
pub(crate) mod test_support {
    /// One entry: header plus data.
    pub(crate) fn entry(type_id: u32, entry_id: u32, flag: bool, data: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(type_id.to_le_bytes());
        bytes.extend(entry_id.to_le_bytes());
        let size = u32::try_from(data.len()).unwrap() | if flag { 0x8000_0000 } else { 0 };
        bytes.extend(size.to_le_bytes());
        bytes.extend(0_u32.to_le_bytes());
        bytes.extend(data);
        bytes
    }

    /// A complete payload: the entries and the closing word.
    pub(crate) fn payload(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = entries.concat();
        bytes.extend([1, 0, 0, 0]);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{entry, payload};
    use super::*;

    #[test]
    fn splits_entries_and_keeps_every_header_word() {
        let bytes = payload(&[
            entry(0x5709_0000, 0, false, &[1, 2, 3]),
            entry(0x56ff_0000, 2, true, &[]),
            entry(0x570b_0004, 7, true, &[9; 40]),
        ]);
        let entries = decode_v8_auxiliary_entries(&bytes, 3).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.index, entry.offset, entry.type_id, entry.entry_id))
                .collect::<Vec<_>>(),
            [
                (0, 0, 0x5709_0000, 0),
                (1, 19, 0x56ff_0000, 2),
                (2, 35, 0x570b_0004, 7)
            ]
        );
        assert_eq!(entries[0].data(), [1, 2, 3]);
        assert_eq!(entries[0].size_word, 3);
        assert!(!entries[0].size_flag());
        assert!(entries[1].data().is_empty());
        assert_eq!(entries[1].size_word, 0x8000_0000);
        assert!(entries[1].size_flag());
        assert_eq!(entries[2].data(), [9; 40]);
        assert_eq!(
            (entries[2].type_major(), entries[2].type_minor()),
            (0x570b, 4)
        );
        assert_eq!(entries[2].reserved, 0);
    }

    #[test]
    fn entries_compare_by_their_own_words_and_data() {
        let bytes = payload(&[
            entry(0x5709_0000, 0, false, &[1, 2, 3]),
            entry(0x56ff_0000, 2, true, &[4, 5]),
        ]);
        let alone = decode_v8_auxiliary_entries(&bytes, 2).unwrap();
        // the same payload inside a larger page with other bytes around it
        let mut page = vec![0xaa; 7];
        page.extend(&bytes);
        page.extend([0xbb; 9]);
        let in_page = split_entries(Arc::from(page), 7..7 + bytes.len(), 2).unwrap();
        assert_eq!(alone, in_page);

        let differs = |change: fn(&mut Vec<u8>)| {
            let mut other = bytes.clone();
            change(&mut other);
            decode_v8_auxiliary_entries(&other, 2).unwrap() != alone
        };
        assert!(differs(|bytes| bytes[16] = 9)); // a data byte
        assert!(differs(|bytes| bytes[4] = 1)); // the entry ID
        assert!(differs(|bytes| bytes[0] = 1)); // the type word
        assert!(differs(|bytes| bytes[12] = 1)); // the reserved word
        assert!(differs(|bytes| bytes[11] = 0x80)); // the flag of the size word
    }

    #[test]
    fn a_record_without_entries_is_only_the_closing_word() {
        assert_eq!(decode_v8_auxiliary_entries(&[1, 0, 0, 0], 0), Some(vec![]));
    }

    #[test]
    fn a_payload_with_more_entries_than_the_limit_stays_raw() {
        // empty entries: a 16-byte header each, then the closing word
        let many = |count: usize| {
            let mut bytes = entry(0x5709_0000, 0, false, &[]).repeat(count);
            bytes.extend([1, 0, 0, 0]);
            (bytes, u32::try_from(count).unwrap())
        };
        let (bytes, count) = many(V8_MAX_AUXILIARY_ENTRIES);
        let entries = decode_v8_auxiliary_entries(&bytes, count).unwrap();
        assert_eq!(entries.len(), V8_MAX_AUXILIARY_ENTRIES);
        // one more entry is a well-formed payload that is not split
        let (bytes, count) = many(V8_MAX_AUXILIARY_ENTRIES + 1);
        assert!(decode_v8_auxiliary_entries(&bytes, count).is_none());
    }

    #[test]
    fn payloads_that_do_not_match_the_layout_stay_raw() {
        let good = payload(&[entry(0x5709_0000, 0, false, &[1, 2, 3, 4])]);
        assert!(decode_v8_auxiliary_entries(&good, 1).is_some());
        // a wrong entry count, in both directions
        assert!(decode_v8_auxiliary_entries(&good, 0).is_none());
        assert!(decode_v8_auxiliary_entries(&good, 2).is_none());
        assert!(decode_v8_auxiliary_entries(&good, u32::MAX).is_none());
        // a missing, short, long or different closing word
        assert!(decode_v8_auxiliary_entries(&good[..good.len() - 4], 1).is_none());
        assert!(decode_v8_auxiliary_entries(&good[..good.len() - 1], 1).is_none());
        let mut long = good.clone();
        long.push(0);
        assert!(decode_v8_auxiliary_entries(&long, 1).is_none());
        let mut other = good.clone();
        *other.last_mut().unwrap() = 1;
        assert!(decode_v8_auxiliary_entries(&other, 1).is_none());
        // a data length that runs past the payload
        let mut oversized = good.clone();
        oversized[8..12].copy_from_slice(&0x7fff_ffff_u32.to_le_bytes());
        assert!(decode_v8_auxiliary_entries(&oversized, 1).is_none());
        assert!(decode_v8_auxiliary_entries(&[], 0).is_none());
        assert!(decode_v8_auxiliary_entries(&[], 1).is_none());
    }

    #[test]
    fn every_prefix_and_single_byte_mutation_is_panic_free_and_bounded() {
        let good = payload(&[
            entry(0x5709_0000, 0, false, &[1, 2, 3, 4]),
            entry(0x56ff_0000, 1, true, &[5; 9]),
        ]);
        for end in 0..good.len() {
            assert!(decode_v8_auxiliary_entries(&good[..end], 2).is_none());
        }
        for offset in 0..good.len() {
            for value in [0x00, 0x7f, 0x80, 0xff] {
                let mut mutated = good.clone();
                mutated[offset] = value;
                for count in [0, 1, 2, 3, u32::MAX] {
                    if let Some(entries) = decode_v8_auxiliary_entries(&mutated, count) {
                        let consumed: usize =
                            entries.iter().map(|entry| 16 + entry.data().len()).sum();
                        assert_eq!(consumed + 4, mutated.len());
                    }
                }
            }
        }
    }
}
