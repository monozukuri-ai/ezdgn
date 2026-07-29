use std::ops::Range;
use std::sync::Arc;

use crate::DgnError;

use super::compression::inflate_zlib_bounded;
use super::{read_v8_streams, V8ContainerInfo, V8ReadOptions, V8Stream, V8StreamSet};

const PAGE_HEADER_BYTES: usize = 16;
const OBJECT_FRAMING_BYTES: usize = 4;
const OBJECT_HEADER_BYTES: usize = 12;
const AUXILIARY_HEADER_BYTES: usize = 28;
const AUXILIARY_MAGIC: u32 = 0x0000_a11b;
const MODEL_INDEX_MAGIC: u32 = 0xaa00_ba11;
const MODEL_INDEX_HEADER_BYTES: usize = 16;
const MODEL_INDEX_ENTRY_HEADER_BYTES: usize = 32;

/// Default maximum number of model, graphical, control, name, and auxiliary pages.
pub const DEFAULT_MAX_V8_PAGES: usize = 16_384;
/// Default maximum number of raw V8 objects across all scanned page families.
pub const DEFAULT_MAX_V8_OBJECTS: usize = 1_000_000;
/// Default maximum byte length of one raw object or auxiliary record (256 KiB).
pub const DEFAULT_MAX_V8_OBJECT_SIZE_BYTES: usize = 256 * 1024;
/// Default maximum inflated byte length of one V8 payload (64 MiB).
pub const DEFAULT_MAX_V8_INFLATED_STREAM_BYTES: usize = 64 * 1024 * 1024;
/// Default maximum combined inflated size of all V8 payloads (1 GiB).
pub const DEFAULT_MAX_V8_TOTAL_INFLATED_BYTES: usize = 1024 * 1024 * 1024;
/// Default maximum number of models accepted from the model index.
pub const DEFAULT_MAX_V8_MODELS: usize = 4096;
/// Default maximum encoded size of a model name or description (1 MiB).
pub const DEFAULT_MAX_V8_STRING_BYTES: usize = 1024 * 1024;
/// Default maximum number of vertices decoded from one V8 element.
pub const DEFAULT_MAX_V8_VERTICES: usize = 100_000;
/// Default maximum complex-element nesting depth.
pub const DEFAULT_MAX_V8_HIERARCHY_DEPTH: usize = 256;

/// Resource limits for raw and semantic V8 scanning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V8ScanOptions {
    pub read: V8ReadOptions,
    pub max_pages: usize,
    pub max_objects: usize,
    pub max_object_size: usize,
    pub max_inflated_stream_size: usize,
    pub max_total_inflated_bytes: usize,
    pub max_models: usize,
    pub max_string_bytes: usize,
    pub max_vertices: usize,
    pub max_hierarchy_depth: usize,
}

impl Default for V8ScanOptions {
    fn default() -> Self {
        Self {
            read: V8ReadOptions::default(),
            max_pages: DEFAULT_MAX_V8_PAGES,
            max_objects: DEFAULT_MAX_V8_OBJECTS,
            max_object_size: DEFAULT_MAX_V8_OBJECT_SIZE_BYTES,
            max_inflated_stream_size: DEFAULT_MAX_V8_INFLATED_STREAM_BYTES,
            max_total_inflated_bytes: DEFAULT_MAX_V8_TOTAL_INFLATED_BYTES,
            max_models: DEFAULT_MAX_V8_MODELS,
            max_string_bytes: DEFAULT_MAX_V8_STRING_BYTES,
            max_vertices: DEFAULT_MAX_V8_VERTICES,
            max_hierarchy_depth: DEFAULT_MAX_V8_HIERARCHY_DEPTH,
        }
    }
}

/// Logical source family for a raw object page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V8ObjectFamily {
    Graphical,
    Control,
    Named,
}

impl V8ObjectFamily {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Graphical => "graphical",
            Self::Control => "control",
            Self::Named => "named",
        }
    }
}

/// Complex-element role encoded by the two high role bits of a V8 type word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V8ObjectRole {
    Standalone,
    Header,
    Component,
    HeaderComponent,
}

impl V8ObjectRole {
    #[must_use]
    pub const fn is_header(self) -> bool {
        matches!(self, Self::Header | Self::HeaderComponent)
    }

    #[must_use]
    pub const fn is_component(self) -> bool {
        matches!(self, Self::Component | Self::HeaderComponent)
    }
}

/// Four little-endian fields prepended to a compressed object/auxiliary page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V8PageHeader {
    pub record_count: u32,
    pub format_version: u32,
    pub page_number: u32,
    pub population: u32,
}

/// One independently bounded raw V8 object.
#[derive(Debug, Clone)]
pub struct V8RawObject {
    pub index: usize,
    pub page_index: usize,
    pub family: V8ObjectFamily,
    pub stream_path: String,
    pub inflated_offset: usize,
    pub framing_prefix: u32,
    pub type_and_flags: u32,
    pub element_type: u16,
    pub role: V8ObjectRole,
    pub words: u32,
    pub attribute_words: u32,
    pub level: Option<u32>,
    pub element_id: Option<u64>,
    pub model_id: Option<u64>,
    bytes: Arc<[u8]>,
    byte_range: Range<usize>,
}

impl V8RawObject {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[self.byte_range.clone()]
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.byte_range.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.byte_range.is_empty()
    }

    /// Bytes at and after the object's attribute-word boundary.
    #[must_use]
    pub fn attribute_bytes(&self) -> &[u8] {
        let bytes = self.as_bytes();
        let offset = usize::try_from(self.attribute_words)
            .unwrap_or(usize::MAX)
            .saturating_mul(2)
            .min(bytes.len());
        &bytes[offset..]
    }

    /// Bytes before the attribute-word boundary.
    #[must_use]
    pub fn primary_bytes(&self) -> &[u8] {
        let bytes = self.as_bytes();
        let end = usize::try_from(self.attribute_words)
            .unwrap_or(usize::MAX)
            .saturating_mul(2)
            .min(bytes.len());
        &bytes[..end]
    }
}

/// A decoded, exact object page with its inflated storage retained by its objects.
#[derive(Debug, Clone)]
pub struct V8ObjectPage {
    pub stream_path: String,
    pub header: V8PageHeader,
    pub family: V8ObjectFamily,
    pub objects: Vec<V8RawObject>,
    pub inflated_size: usize,
}

/// One auxiliary/XAttribute record associated with an element identifier.
#[derive(Debug, Clone)]
pub struct V8AuxiliaryRecord {
    pub index: usize,
    pub stream_path: String,
    pub inflated_offset: usize,
    pub magic: u32,
    pub kind: u32,
    pub reserved: u32,
    pub element_id: u64,
    pub flags: u32,
    bytes: Arc<[u8]>,
    byte_range: Range<usize>,
    payload_range: Range<usize>,
}

impl V8AuxiliaryRecord {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[self.byte_range.clone()]
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.bytes[self.payload_range.clone()]
    }
}

/// A decoded auxiliary page.
#[derive(Debug, Clone)]
pub struct V8AuxiliaryPage {
    pub stream_path: String,
    pub header: V8PageHeader,
    pub records: Vec<V8AuxiliaryRecord>,
    pub inflated_size: usize,
}

/// One variable-length entry from `/Dgn^Ix/Dgn~Mix`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V8ModelIndexEntry {
    pub index: usize,
    pub raw_number: u32,
    pub storage_index: u16,
    pub model_number: u16,
    pub flags: u32,
    pub model_id: u64,
    pub name: String,
    pub description: String,
    pub raw_bytes: Arc<[u8]>,
}

/// Raw object population associated with one model storage.
#[derive(Debug, Clone)]
pub struct V8RawModel {
    pub index: V8ModelIndexEntry,
    pub storage_path: String,
    pub model_header_stream: V8Stream,
    pub model_header_bytes: Arc<[u8]>,
    pub graphical_pages: Vec<V8ObjectPage>,
    pub graphical_auxiliary_pages: Vec<V8AuxiliaryPage>,
    pub control_pages: Vec<V8ObjectPage>,
    pub control_auxiliary_pages: Vec<V8AuxiliaryPage>,
}

impl V8RawModel {
    pub fn graphical_objects(&self) -> impl Iterator<Item = &V8RawObject> {
        self.graphical_pages
            .iter()
            .flat_map(|page| page.objects.iter())
    }

    pub fn control_objects(&self) -> impl Iterator<Item = &V8RawObject> {
        self.control_pages
            .iter()
            .flat_map(|page| page.objects.iter())
    }
}

/// Complete bounded raw scan of the known V8 object page families.
#[derive(Debug, Clone)]
pub struct V8RawDocument {
    pub container: V8ContainerInfo,
    pub models: Vec<V8RawModel>,
    pub named_pages: Vec<V8ObjectPage>,
    pub named_auxiliary_pages: Vec<V8AuxiliaryPage>,
    pub total_inflated_bytes: usize,
}

impl V8RawDocument {
    #[must_use]
    pub fn graphical_object_count(&self) -> usize {
        self.models
            .iter()
            .flat_map(|model| model.graphical_pages.iter())
            .map(|page| page.objects.len())
            .sum()
    }

    #[must_use]
    pub fn total_object_count(&self) -> usize {
        let model_objects = self
            .models
            .iter()
            .flat_map(|model| {
                model
                    .graphical_pages
                    .iter()
                    .chain(model.control_pages.iter())
            })
            .map(|page| page.objects.len())
            .sum::<usize>();
        model_objects
            + self
                .named_pages
                .iter()
                .map(|page| page.objects.len())
                .sum::<usize>()
    }
}

/// Read the model index and every recognized V8 object/auxiliary page.
pub fn scan_v8_objects(input: &[u8], options: V8ScanOptions) -> Result<V8RawDocument, DgnError> {
    let streams = read_v8_streams(input, options.read)?;
    scan_v8_streams(streams, options)
}

fn scan_v8_streams(
    streams: V8StreamSet,
    options: V8ScanOptions,
) -> Result<V8RawDocument, DgnError> {
    let mut budget = ScanBudget::new(options);
    let index_stream =
        streams
            .get("/Dgn^Ix/Dgn~Mix")
            .ok_or_else(|| DgnError::InvalidV8ModelIndex {
                offset: 0,
                context: "missing /Dgn^Ix/Dgn~Mix stream".to_owned(),
            })?;
    let index_bytes = inflate_and_charge(index_stream, 0, &mut budget)?;
    let model_index = parse_model_index(&index_bytes, options)?;

    let mut models = Vec::with_capacity(model_index.len());
    for entry in model_index {
        let storage_path = format!("/Dgn-Md/#{:06}", entry.storage_index);
        let model_header_path = format!("{storage_path}/Dgn~Mh");
        let model_header_stream = streams.get(&model_header_path).cloned().ok_or_else(|| {
            DgnError::InvalidV8ModelHeader {
                path: model_header_path.clone(),
                context: "stream is missing".to_owned(),
            }
        })?;
        let model_header_bytes: Arc<[u8]> =
            Arc::from(inflate_and_charge(&model_header_stream, 0, &mut budget)?);

        let graphical_pages = scan_object_family(
            &streams,
            &format!("{storage_path}/Dgn^G/"),
            V8ObjectFamily::Graphical,
            &mut budget,
        )?;
        let graphical_auxiliary_pages =
            scan_auxiliary_family(&streams, &format!("{storage_path}/Dgn^GA/"), &mut budget)?;
        let control_pages = scan_object_family(
            &streams,
            &format!("{storage_path}/Dgn^C/"),
            V8ObjectFamily::Control,
            &mut budget,
        )?;
        let control_auxiliary_pages =
            scan_auxiliary_family(&streams, &format!("{storage_path}/Dgn^CA/"), &mut budget)?;
        models.push(V8RawModel {
            index: entry,
            storage_path,
            model_header_stream,
            model_header_bytes,
            graphical_pages,
            graphical_auxiliary_pages,
            control_pages,
            control_auxiliary_pages,
        });
    }

    let named_pages = scan_object_family(&streams, "/Dgn^Nm/", V8ObjectFamily::Named, &mut budget)?;
    let named_auxiliary_pages = scan_auxiliary_family(&streams, "/Dgn^NmA/", &mut budget)?;

    Ok(V8RawDocument {
        container: streams.container,
        models,
        named_pages,
        named_auxiliary_pages,
        total_inflated_bytes: budget.total_inflated,
    })
}

#[derive(Debug)]
struct ScanBudget {
    options: V8ScanOptions,
    pages: usize,
    objects: usize,
    total_inflated: usize,
}

impl ScanBudget {
    const fn new(options: V8ScanOptions) -> Self {
        Self {
            options,
            pages: 0,
            objects: 0,
            total_inflated: 0,
        }
    }

    fn add_page(&mut self) -> Result<(), DgnError> {
        self.pages = self
            .pages
            .checked_add(1)
            .ok_or(DgnError::V8PageLimitExceeded {
                limit: self.options.max_pages,
            })?;
        if self.pages > self.options.max_pages {
            return Err(DgnError::V8PageLimitExceeded {
                limit: self.options.max_pages,
            });
        }
        Ok(())
    }

    fn reserve_objects(&mut self, count: usize) -> Result<usize, DgnError> {
        let first = self.objects;
        self.objects = self
            .objects
            .checked_add(count)
            .ok_or(DgnError::V8ObjectLimitExceeded {
                limit: self.options.max_objects,
            })?;
        if self.objects > self.options.max_objects {
            return Err(DgnError::V8ObjectLimitExceeded {
                limit: self.options.max_objects,
            });
        }
        Ok(first)
    }

    fn charge_inflated(&mut self, size: usize) -> Result<(), DgnError> {
        self.total_inflated = self.total_inflated.checked_add(size).ok_or(
            DgnError::V8TotalInflatedSizeLimitExceeded {
                limit: self.options.max_total_inflated_bytes,
            },
        )?;
        if self.total_inflated > self.options.max_total_inflated_bytes {
            return Err(DgnError::V8TotalInflatedSizeLimitExceeded {
                limit: self.options.max_total_inflated_bytes,
            });
        }
        Ok(())
    }
}

fn inflate_and_charge(
    stream: &V8Stream,
    offset: usize,
    budget: &mut ScanBudget,
) -> Result<Vec<u8>, DgnError> {
    let bytes = inflate_zlib_bounded(
        &stream.path,
        stream.as_bytes(),
        offset,
        budget.options.max_inflated_stream_size,
    )?;
    budget.charge_inflated(bytes.len())?;
    Ok(bytes)
}

fn parse_model_index(
    bytes: &[u8],
    options: V8ScanOptions,
) -> Result<Vec<V8ModelIndexEntry>, DgnError> {
    if bytes.len() < MODEL_INDEX_HEADER_BYTES {
        return Err(model_index_error(0, "header is truncated"));
    }
    if read_u32(bytes, 0) != Some(MODEL_INDEX_MAGIC) {
        return Err(model_index_error(0, "unexpected index magic"));
    }
    let version = read_u32(bytes, 4).unwrap_or_default();
    if version != 4 {
        return Err(model_index_error(
            4,
            format!("unsupported index version {version}"),
        ));
    }
    let count = usize::try_from(read_u32(bytes, 8).unwrap_or_default()).unwrap_or(usize::MAX);
    if count > options.max_models {
        return Err(DgnError::V8ObjectLimitExceeded {
            limit: options.max_models,
        });
    }

    let mut entries = Vec::with_capacity(count);
    let mut offset = MODEL_INDEX_HEADER_BYTES;
    for index in 0..count {
        let header = bytes
            .get(offset..offset.saturating_add(MODEL_INDEX_ENTRY_HEADER_BYTES))
            .ok_or_else(|| model_index_error(offset, "entry header is truncated"))?;
        let raw_number = read_u32(header, 0).unwrap_or_default();
        let flags = read_u32(header, 4).unwrap_or_default();
        let model_id = read_u64(header, 8).unwrap_or_default();
        let entry_size = usize::from(read_u16(header, 16).unwrap_or_default());
        let name_bytes = usize::from(read_u16(header, 18).unwrap_or_default());
        let description_bytes =
            usize::try_from(read_u32(header, 20).unwrap_or_default()).unwrap_or(usize::MAX);
        if entry_size < MODEL_INDEX_ENTRY_HEADER_BYTES {
            return Err(model_index_error(offset + 16, "entry size is too small"));
        }
        if name_bytes % 2 != 0 || description_bytes % 2 != 0 {
            return Err(model_index_error(
                offset + 18,
                "UTF-16 name or description has an odd byte length",
            ));
        }
        for (context, size) in [
            ("model name", name_bytes),
            ("model description", description_bytes),
        ] {
            if size > options.max_string_bytes {
                return Err(DgnError::V8StringLimitExceeded {
                    context: context.to_owned(),
                    actual: size,
                    limit: options.max_string_bytes,
                });
            }
        }
        let string_bytes = name_bytes
            .checked_add(description_bytes)
            .ok_or_else(|| model_index_error(offset + 18, "string lengths overflow"))?;
        let required = MODEL_INDEX_ENTRY_HEADER_BYTES
            .checked_add(string_bytes)
            .ok_or_else(|| model_index_error(offset + 16, "entry length overflows"))?;
        if required > entry_size {
            return Err(model_index_error(
                offset + 16,
                "entry is shorter than its declared strings",
            ));
        }
        let end = offset
            .checked_add(entry_size)
            .ok_or_else(|| model_index_error(offset, "entry end overflows"))?;
        let entry = bytes
            .get(offset..end)
            .ok_or_else(|| model_index_error(offset, "entry extends past the stream"))?;
        let name_start = MODEL_INDEX_ENTRY_HEADER_BYTES;
        let description_start = name_start + name_bytes;
        let name = decode_utf16(
            &entry[name_start..description_start],
            offset + name_start,
            "model name",
        )?;
        let description = decode_utf16(
            &entry[description_start..description_start + description_bytes],
            offset + description_start,
            "model description",
        )?;
        entries.push(V8ModelIndexEntry {
            index,
            raw_number,
            storage_index: raw_number as u16,
            model_number: (raw_number >> 16) as u16,
            flags,
            model_id,
            name,
            description,
            raw_bytes: Arc::from(entry),
        });
        offset = end;
    }
    if offset != bytes.len() {
        return Err(model_index_error(
            offset,
            format!("{} trailing bytes remain", bytes.len() - offset),
        ));
    }
    Ok(entries)
}

fn decode_utf16(bytes: &[u8], offset: usize, context: &'static str) -> Result<String, DgnError> {
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&units)
        .map_err(|error| model_index_error(offset, format!("invalid UTF-16 {context}: {error}")))
}

fn model_index_error(offset: usize, context: impl Into<String>) -> DgnError {
    DgnError::InvalidV8ModelIndex {
        offset,
        context: context.into(),
    }
}

fn scan_object_family(
    streams: &V8StreamSet,
    prefix: &str,
    family: V8ObjectFamily,
    budget: &mut ScanBudget,
) -> Result<Vec<V8ObjectPage>, DgnError> {
    let pages = numbered_streams(streams, prefix)?;
    let mut decoded = Vec::with_capacity(pages.len());
    for (page_number, stream) in pages {
        budget.add_page()?;
        decoded.push(scan_object_page(stream, page_number, family, budget)?);
    }
    Ok(decoded)
}

fn scan_auxiliary_family(
    streams: &V8StreamSet,
    prefix: &str,
    budget: &mut ScanBudget,
) -> Result<Vec<V8AuxiliaryPage>, DgnError> {
    let pages = numbered_streams(streams, prefix)?;
    let mut decoded = Vec::with_capacity(pages.len());
    for (page_number, stream) in pages {
        budget.add_page()?;
        decoded.push(scan_auxiliary_page(stream, page_number, budget)?);
    }
    Ok(decoded)
}

fn numbered_streams<'a>(
    streams: &'a V8StreamSet,
    prefix: &str,
) -> Result<Vec<(u32, &'a V8Stream)>, DgnError> {
    let mut pages = streams
        .streams
        .iter()
        .filter_map(|stream| {
            let suffix = stream.path.strip_prefix(prefix)?;
            let number = suffix.strip_prefix('$')?.parse::<u32>().ok()?;
            (!suffix[1..].contains('/')).then_some((number, stream))
        })
        .collect::<Vec<_>>();
    pages.sort_by_key(|(number, _)| *number);
    for pair in pages.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(DgnError::InvalidV8Page {
                path: prefix.to_owned(),
                offset: 0,
                context: format!("duplicate page number {}", pair[0].0),
            });
        }
    }
    Ok(pages)
}

fn page_header(stream: &V8Stream) -> Result<V8PageHeader, DgnError> {
    if stream.len() < PAGE_HEADER_BYTES {
        return Err(page_error(
            &stream.path,
            0,
            format!("page header is only {} bytes", stream.len()),
        ));
    }
    let header = V8PageHeader {
        record_count: read_u32(stream.as_bytes(), 0).unwrap_or_default(),
        format_version: read_u32(stream.as_bytes(), 4).unwrap_or_default(),
        page_number: read_u32(stream.as_bytes(), 8).unwrap_or_default(),
        population: read_u32(stream.as_bytes(), 12).unwrap_or_default(),
    };
    if !matches!(header.format_version, 2 | 3) {
        return Err(page_error(
            &stream.path,
            4,
            format!("unsupported page format version {}", header.format_version),
        ));
    }
    Ok(header)
}

fn inflate_page(
    stream: &V8Stream,
    header: V8PageHeader,
    budget: &mut ScanBudget,
) -> Result<Arc<[u8]>, DgnError> {
    let inflated = if stream.len() == PAGE_HEADER_BYTES {
        if header.record_count != 0 {
            return Err(page_error(
                &stream.path,
                PAGE_HEADER_BYTES,
                "non-empty page has no compressed payload",
            ));
        }
        Vec::new()
    } else {
        inflate_and_charge(stream, PAGE_HEADER_BYTES, budget)?
    };
    if stream.len() == PAGE_HEADER_BYTES {
        budget.charge_inflated(0)?;
    }
    Ok(Arc::from(inflated))
}

fn scan_object_page(
    stream: &V8Stream,
    page_number: u32,
    family: V8ObjectFamily,
    budget: &mut ScanBudget,
) -> Result<V8ObjectPage, DgnError> {
    let header = page_header(stream)?;
    // Format-3 control/name pages observed in independently published files
    // use zero for these two advisory fields; graphical pages and format-2
    // pages carry the explicit page/count values.
    if header.page_number != 0 && header.page_number != page_number {
        return Err(page_error(
            &stream.path,
            8,
            format!(
                "path page ${page_number} disagrees with header page {}",
                header.page_number
            ),
        ));
    }
    if header.population != 0 && header.population != header.record_count {
        return Err(page_error(
            &stream.path,
            12,
            format!(
                "population {} disagrees with record count {}",
                header.population, header.record_count
            ),
        ));
    }
    let count = usize::try_from(header.record_count).unwrap_or(usize::MAX);
    let first_index = budget.reserve_objects(count)?;
    let bytes = inflate_page(stream, header, budget)?;
    let mut objects = Vec::with_capacity(count);
    let mut offset = 0usize;
    for page_index in 0..count {
        let framing_end = offset
            .checked_add(OBJECT_FRAMING_BYTES)
            .ok_or_else(|| page_error(&stream.path, offset, "object framing offset overflows"))?;
        let framing = bytes
            .get(offset..framing_end)
            .ok_or_else(|| page_error(&stream.path, offset, "object framing is truncated"))?;
        let record_offset = framing_end;
        let header_end = record_offset
            .checked_add(OBJECT_HEADER_BYTES)
            .ok_or_else(|| page_error(&stream.path, record_offset, "object header overflows"))?;
        let object_header = bytes
            .get(record_offset..header_end)
            .ok_or_else(|| page_error(&stream.path, record_offset, "object header is truncated"))?;
        let type_and_flags = read_u32(object_header, 0).unwrap_or_default();
        let words = read_u32(object_header, 4).unwrap_or_default();
        let attribute_words = read_u32(object_header, 8).unwrap_or_default();
        let element_type = (type_and_flags & 0xffff) as u16;
        let declared = usize::try_from(words)
            .unwrap_or(usize::MAX)
            .checked_mul(2)
            .ok_or_else(|| page_error(&stream.path, record_offset + 4, "object size overflows"))?;
        if declared < OBJECT_HEADER_BYTES {
            return Err(page_error(
                &stream.path,
                record_offset + 4,
                format!("object type {element_type} declares only {declared} bytes"),
            ));
        }
        if declared > budget.options.max_object_size {
            return Err(DgnError::V8ObjectSizeLimitExceeded {
                path: stream.path.clone(),
                offset: record_offset,
                element_type,
                declared,
                limit: budget.options.max_object_size,
            });
        }
        if attribute_words > words {
            return Err(page_error(
                &stream.path,
                record_offset + 8,
                format!(
                    "attribute boundary {attribute_words} words exceeds object size {words} words"
                ),
            ));
        }
        let record_end = record_offset
            .checked_add(declared)
            .ok_or_else(|| page_error(&stream.path, record_offset, "object end overflows"))?;
        let record = bytes.get(record_offset..record_end).ok_or_else(|| {
            page_error(
                &stream.path,
                record_offset,
                format!(
                    "object type {element_type} declares {declared} bytes, but only {} remain",
                    bytes.len().saturating_sub(record_offset)
                ),
            )
        })?;
        let role_bits = type_and_flags & 0x6000_0000;
        let role = match role_bits {
            0x2000_0000 => V8ObjectRole::Header,
            0x4000_0000 => V8ObjectRole::Component,
            0x6000_0000 => V8ObjectRole::HeaderComponent,
            _ => V8ObjectRole::Standalone,
        };
        objects.push(V8RawObject {
            index: first_index + page_index,
            page_index,
            family,
            stream_path: stream.path.clone(),
            inflated_offset: record_offset,
            framing_prefix: read_u32(framing, 0).unwrap_or_default(),
            type_and_flags,
            element_type,
            role,
            words,
            attribute_words,
            level: read_u32(record, 12),
            element_id: read_u64(record, 16),
            model_id: read_u64(record, 24),
            bytes: Arc::clone(&bytes),
            byte_range: record_offset..record_end,
        });
        offset = record_end;
    }
    if offset != bytes.len() {
        return Err(page_error(
            &stream.path,
            offset,
            format!("{} trailing inflated bytes remain", bytes.len() - offset),
        ));
    }
    Ok(V8ObjectPage {
        stream_path: stream.path.clone(),
        header,
        family,
        objects,
        inflated_size: bytes.len(),
    })
}

fn scan_auxiliary_page(
    stream: &V8Stream,
    _page_number: u32,
    budget: &mut ScanBudget,
) -> Result<V8AuxiliaryPage, DgnError> {
    let header = page_header(stream)?;
    let count = usize::try_from(header.record_count).unwrap_or(usize::MAX);
    let first_index = budget.reserve_objects(count)?;
    let bytes = inflate_page(stream, header, budget)?;
    let mut records = Vec::with_capacity(count);
    let mut offset = 0usize;
    for index in 0..count {
        let header_end = offset
            .checked_add(AUXILIARY_HEADER_BYTES)
            .ok_or_else(|| page_error(&stream.path, offset, "auxiliary header offset overflows"))?;
        let record_header = bytes.get(offset..header_end).ok_or_else(|| {
            page_error(&stream.path, offset, "auxiliary record header is truncated")
        })?;
        let magic = read_u32(record_header, 0).unwrap_or_default();
        if magic != AUXILIARY_MAGIC {
            return Err(page_error(
                &stream.path,
                offset,
                format!("unexpected auxiliary magic 0x{magic:08x}"),
            ));
        }
        let payload_size =
            usize::try_from(read_u32(record_header, 4).unwrap_or_default()).unwrap_or(usize::MAX);
        let record_size = AUXILIARY_HEADER_BYTES
            .checked_add(payload_size)
            .ok_or_else(|| page_error(&stream.path, offset + 4, "auxiliary size overflows"))?;
        if record_size > budget.options.max_object_size {
            return Err(DgnError::V8ObjectSizeLimitExceeded {
                path: stream.path.clone(),
                offset,
                element_type: 0,
                declared: record_size,
                limit: budget.options.max_object_size,
            });
        }
        let end = offset
            .checked_add(record_size)
            .ok_or_else(|| page_error(&stream.path, offset, "auxiliary end overflows"))?;
        bytes.get(offset..end).ok_or_else(|| {
            page_error(
                &stream.path,
                offset,
                format!(
                    "auxiliary record declares {record_size} bytes, but only {} remain",
                    bytes.len().saturating_sub(offset)
                ),
            )
        })?;
        records.push(V8AuxiliaryRecord {
            index: first_index + index,
            stream_path: stream.path.clone(),
            inflated_offset: offset,
            magic,
            kind: read_u32(record_header, 8).unwrap_or_default(),
            reserved: read_u32(record_header, 12).unwrap_or_default(),
            element_id: read_u64(record_header, 16).unwrap_or_default(),
            flags: read_u32(record_header, 24).unwrap_or_default(),
            bytes: Arc::clone(&bytes),
            byte_range: offset..end,
            payload_range: header_end..end,
        });
        offset = end;
    }
    if offset != bytes.len() {
        return Err(page_error(
            &stream.path,
            offset,
            format!("{} trailing inflated bytes remain", bytes.len() - offset),
        ));
    }
    Ok(V8AuxiliaryPage {
        stream_path: stream.path.clone(),
        header,
        records,
        inflated_size: bytes.len(),
    })
}

fn page_error(path: &str, offset: usize, context: impl Into<String>) -> DgnError {
    DgnError::InvalidV8Page {
        path: path.to_owned(),
        offset,
        context: context.into(),
    }
}

pub(crate) fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let value = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([value[0], value[1]]))
}

pub(crate) fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let value = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

pub(crate) fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let value = bytes.get(offset..offset.checked_add(8)?)?;
    Some(u64::from_le_bytes([
        value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
    ]))
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use flate2::{write::ZlibEncoder, Compression};

    use super::*;

    fn zlib(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn object(type_and_flags: u32, level: u32, id: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(type_and_flags.to_le_bytes());
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(level.to_le_bytes());
        bytes.extend(id.to_le_bytes());
        bytes.extend(7_u64.to_le_bytes());
        bytes
    }

    fn dgn_with_one_object() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut cfb = cfb::CompoundFile::create_with_version(cfb::Version::V3, cursor).unwrap();
        for storage in [
            "/Dgn-Md",
            "/Dgn-Md/#000000",
            "/Dgn-Md/#000000/Dgn^G",
            "/Dgn^Ix",
        ] {
            cfb.create_storage(storage).unwrap();
        }
        let mut file_header = cfb.create_stream("/Dgn~H").unwrap();
        file_header.write_all(b"header").unwrap();
        drop(file_header);
        let mut summary = cfb.create_stream("/Dgn~S").unwrap();
        summary.write_all(b"summary").unwrap();
        drop(summary);

        let mut index = Vec::new();
        index.extend(MODEL_INDEX_MAGIC.to_le_bytes());
        index.extend(4_u32.to_le_bytes());
        index.extend(1_u32.to_le_bytes());
        index.extend(0_u32.to_le_bytes());
        index.extend(0x0001_0000_u32.to_le_bytes());
        index.extend(0_u32.to_le_bytes());
        index.extend(7_u64.to_le_bytes());
        index.extend(36_u16.to_le_bytes());
        index.extend(2_u16.to_le_bytes());
        index.extend(2_u32.to_le_bytes());
        index.extend(0_u64.to_le_bytes());
        index.extend([b'm', 0, b'd', 0]);
        let mut index_stream = cfb.create_stream("/Dgn^Ix/Dgn~Mix").unwrap();
        index_stream.write_all(&zlib(&index)).unwrap();
        drop(index_stream);

        let mut model_header = cfb.create_stream("/Dgn-Md/#000000/Dgn~Mh").unwrap();
        model_header.write_all(&zlib(&[0; 32])).unwrap();
        drop(model_header);

        let object = object(0x1000_0003, 64, 42);
        let mut inflated = 0_u32.to_le_bytes().to_vec();
        inflated.extend(object);
        let mut page = Vec::new();
        page.extend(1_u32.to_le_bytes());
        page.extend(2_u32.to_le_bytes());
        page.extend(1_u32.to_le_bytes());
        page.extend(1_u32.to_le_bytes());
        page.extend(zlib(&inflated));
        let mut page_stream = cfb.create_stream("/Dgn-Md/#000000/Dgn^G/$1").unwrap();
        page_stream.write_all(&page).unwrap();
        drop(page_stream);
        cfb.into_inner().into_inner()
    }

    fn object_page_stream(
        count: u32,
        version: u32,
        page_number: u32,
        population: u32,
        inflated: &[u8],
    ) -> V8Stream {
        let mut bytes = Vec::new();
        bytes.extend(count.to_le_bytes());
        bytes.extend(version.to_le_bytes());
        bytes.extend(page_number.to_le_bytes());
        bytes.extend(population.to_le_bytes());
        bytes.extend(zlib(inflated));
        V8Stream {
            path: "/Dgn-Md/#000000/Dgn^G/$1".to_owned(),
            bytes: Arc::from(bytes),
        }
    }

    fn valid_inflated_object() -> Vec<u8> {
        let mut inflated = 0_u32.to_le_bytes().to_vec();
        inflated.extend(object(0x1000_0003, 64, 42));
        inflated
    }

    #[test]
    fn scans_model_index_and_object_page_without_absolute_object_offsets() {
        let raw = scan_v8_objects(&dgn_with_one_object(), V8ScanOptions::default()).unwrap();
        assert_eq!(raw.models.len(), 1);
        assert_eq!(raw.models[0].index.name, "m");
        assert_eq!(raw.models[0].index.description, "d");
        let objects = raw.models[0].graphical_objects().collect::<Vec<_>>();
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].element_type, 3);
        assert_eq!(objects[0].level, Some(64));
        assert_eq!(objects[0].element_id, Some(42));
        assert_eq!(objects[0].len(), 32);
    }

    #[test]
    fn rejects_invalid_page_headers_and_compressed_members() {
        let valid = valid_inflated_object();
        for (stream, expected_offset) in [
            (object_page_stream(1, 9, 1, 1, &valid), 4),
            (object_page_stream(1, 2, 2, 1, &valid), 8),
            (object_page_stream(1, 2, 1, 2, &valid), 12),
        ] {
            let error = scan_object_page(
                &stream,
                1,
                V8ObjectFamily::Graphical,
                &mut ScanBudget::new(V8ScanOptions::default()),
            )
            .unwrap_err();
            assert!(matches!(
                error,
                DgnError::InvalidV8Page { offset, .. } if offset == expected_offset
            ));
        }

        let mut corrupt = object_page_stream(1, 2, 1, 1, &valid);
        corrupt.bytes = Arc::from(&corrupt.as_bytes()[..corrupt.len() - 1]);
        assert!(matches!(
            scan_object_page(
                &corrupt,
                1,
                V8ObjectFamily::Graphical,
                &mut ScanBudget::new(V8ScanOptions::default()),
            ),
            Err(DgnError::InvalidV8CompressedStream { .. })
        ));
    }

    #[test]
    fn rejects_truncated_inconsistent_and_oversized_object_frames() {
        let cases = [
            (vec![], "object framing is truncated"),
            (vec![0; 8], "object header is truncated"),
            (
                {
                    let mut bytes = 0_u32.to_le_bytes().to_vec();
                    bytes.extend(3_u32.to_le_bytes());
                    bytes.extend(5_u32.to_le_bytes());
                    bytes.extend(5_u32.to_le_bytes());
                    bytes
                },
                "declares only 10 bytes",
            ),
            (
                {
                    let mut bytes = 0_u32.to_le_bytes().to_vec();
                    bytes.extend(3_u32.to_le_bytes());
                    bytes.extend(16_u32.to_le_bytes());
                    bytes.extend(17_u32.to_le_bytes());
                    bytes.extend([0; 20]);
                    bytes
                },
                "attribute boundary 17 words exceeds",
            ),
            (
                {
                    let mut bytes = valid_inflated_object();
                    bytes.push(0);
                    bytes
                },
                "trailing inflated bytes remain",
            ),
        ];
        for (inflated, message) in cases {
            let stream = object_page_stream(1, 2, 1, 1, &inflated);
            let error = scan_object_page(
                &stream,
                1,
                V8ObjectFamily::Graphical,
                &mut ScanBudget::new(V8ScanOptions::default()),
            )
            .unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
        }

        let stream = object_page_stream(1, 2, 1, 1, &valid_inflated_object());
        let options = V8ScanOptions {
            max_object_size: 31,
            ..V8ScanOptions::default()
        };
        assert!(matches!(
            scan_object_page(
                &stream,
                1,
                V8ObjectFamily::Graphical,
                &mut ScanBudget::new(options),
            ),
            Err(DgnError::V8ObjectSizeLimitExceeded { declared: 32, .. })
        ));
    }

    #[test]
    fn enforces_page_object_and_inflated_budgets() {
        let stream = object_page_stream(1, 2, 1, 1, &valid_inflated_object());
        let mut page_budget = ScanBudget::new(V8ScanOptions {
            max_pages: 0,
            ..V8ScanOptions::default()
        });
        assert!(matches!(
            page_budget.add_page(),
            Err(DgnError::V8PageLimitExceeded { limit: 0 })
        ));

        let object_options = V8ScanOptions {
            max_objects: 0,
            ..V8ScanOptions::default()
        };
        assert!(matches!(
            scan_object_page(
                &stream,
                1,
                V8ObjectFamily::Graphical,
                &mut ScanBudget::new(object_options),
            ),
            Err(DgnError::V8ObjectLimitExceeded { limit: 0 })
        ));

        let inflated_options = V8ScanOptions {
            max_inflated_stream_size: valid_inflated_object().len() - 1,
            ..V8ScanOptions::default()
        };
        assert!(matches!(
            scan_object_page(
                &stream,
                1,
                V8ObjectFamily::Graphical,
                &mut ScanBudget::new(inflated_options),
            ),
            Err(DgnError::V8InflatedSizeLimitExceeded { .. })
        ));
    }

    #[test]
    fn rejects_malformed_auxiliary_records_and_assigns_global_indices() {
        let mut record = Vec::new();
        record.extend(AUXILIARY_MAGIC.to_le_bytes());
        record.extend(2_u32.to_le_bytes());
        record.extend(9_u32.to_le_bytes());
        record.extend(0_u32.to_le_bytes());
        record.extend(42_u64.to_le_bytes());
        record.extend(1_u32.to_le_bytes());
        record.extend([7, 8]);
        let stream = object_page_stream(1, 2, 1, 1, &record);
        let mut budget = ScanBudget::new(V8ScanOptions::default());
        budget.reserve_objects(5).unwrap();
        let page = scan_auxiliary_page(&stream, 1, &mut budget).unwrap();
        assert_eq!(page.records[0].index, 5);
        assert_eq!(page.records[0].payload(), [7, 8]);

        record[0] = 0;
        let stream = object_page_stream(1, 2, 1, 1, &record);
        assert!(matches!(
            scan_auxiliary_page(&stream, 1, &mut ScanBudget::new(V8ScanOptions::default())),
            Err(DgnError::InvalidV8Page { .. })
        ));
    }

    #[test]
    fn model_index_rejects_each_variable_length_boundary() {
        for bytes in [
            vec![],
            {
                let mut bytes = vec![0; MODEL_INDEX_HEADER_BYTES];
                bytes[0..4].copy_from_slice(&MODEL_INDEX_MAGIC.to_le_bytes());
                bytes[4..8].copy_from_slice(&3_u32.to_le_bytes());
                bytes
            },
            {
                let mut bytes = vec![0; MODEL_INDEX_HEADER_BYTES];
                bytes[0..4].copy_from_slice(&MODEL_INDEX_MAGIC.to_le_bytes());
                bytes[4..8].copy_from_slice(&4_u32.to_le_bytes());
                bytes[8..12].copy_from_slice(&1_u32.to_le_bytes());
                bytes
            },
        ] {
            assert!(parse_model_index(&bytes, V8ScanOptions::default()).is_err());
        }

        let options = V8ScanOptions {
            max_models: 0,
            ..V8ScanOptions::default()
        };
        let mut header = vec![0; MODEL_INDEX_HEADER_BYTES];
        header[0..4].copy_from_slice(&MODEL_INDEX_MAGIC.to_le_bytes());
        header[4..8].copy_from_slice(&4_u32.to_le_bytes());
        header[8..12].copy_from_slice(&1_u32.to_le_bytes());
        assert!(matches!(
            parse_model_index(&header, options),
            Err(DgnError::V8ObjectLimitExceeded { limit: 0 })
        ));
    }

    #[test]
    fn every_synthetic_v8_prefix_and_sampled_mutation_is_panic_free() {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let bytes = dgn_with_one_object();
        for end in 0..=bytes.len() {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let _ = scan_v8_objects(&bytes[..end], V8ScanOptions::default());
            }));
            assert!(outcome.is_ok(), "panic at prefix {end}");
        }
        for offset in (0..bytes.len()).step_by(17) {
            let mut mutated = bytes.clone();
            mutated[offset] ^= 0xff;
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let _ = scan_v8_objects(&mutated, V8ScanOptions::default());
            }));
            assert!(outcome.is_ok(), "panic after mutation at {offset}");
        }
    }
}
