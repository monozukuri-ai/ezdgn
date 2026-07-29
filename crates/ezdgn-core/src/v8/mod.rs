//! Bounded raw and semantic reading for DGN V8 CFB containers.
//!
//! Generic CFB access, raw DGN object framing, and semantic element decoding
//! remain separate layers. Unknown objects retain their exact bytes instead of
//! being discarded or flattened.

mod compression;
mod container;
mod entity;
mod linkage;
mod metadata;
mod raw;
mod stream;

pub use container::{
    inspect_v8_container, V8CfbEntry, V8CfbEntryKind, V8ContainerInfo, DEFAULT_MAX_CFB_ENTRIES,
};
pub use entity::{
    read_v8, V8CommonHeader, V8Document, V8Element, V8ElementData, V8Model, V8Point,
    V8PointOrientation, V8TextEncoding,
};
pub use linkage::{decode_v8_linkages, V8Linkage, V8_PROPERTY_LINKAGE_KIND};
pub use metadata::{V8Dimension, V8ModelMetadata, V8Point3, V8Range3, V8Range3I64};
pub use raw::{
    scan_v8_objects, V8AuxiliaryPage, V8AuxiliaryRecord, V8ModelIndexEntry, V8ObjectFamily,
    V8ObjectPage, V8ObjectRole, V8PageHeader, V8RawDocument, V8RawModel, V8RawObject,
    V8ScanOptions, DEFAULT_MAX_V8_HIERARCHY_DEPTH, DEFAULT_MAX_V8_INFLATED_STREAM_BYTES,
    DEFAULT_MAX_V8_MODELS, DEFAULT_MAX_V8_OBJECTS, DEFAULT_MAX_V8_OBJECT_SIZE_BYTES,
    DEFAULT_MAX_V8_PAGES, DEFAULT_MAX_V8_STRING_BYTES, DEFAULT_MAX_V8_TOTAL_INFLATED_BYTES,
    DEFAULT_MAX_V8_VERTICES,
};
pub use stream::{
    read_v8_stream, read_v8_streams, V8ReadOptions, V8Stream, V8StreamSet,
    DEFAULT_MAX_CFB_STREAM_SIZE_BYTES, DEFAULT_MAX_CFB_TOTAL_STREAM_BYTES,
};
