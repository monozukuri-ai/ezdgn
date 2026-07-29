use std::collections::BTreeSet;

use ezdgn_core::{
    read_v8, read_v8_stream, read_v8_streams, scan_v8_objects, DgnError, V8Dimension,
    V8ElementData, V8ObjectRole, V8ReadOptions, V8ScanOptions,
};
use sha2::{Digest, Sha256};

const V8: &[u8] = include_bytes!("../../../tests/data/dgn/v8/test_dgnv8.dgn");

const STREAMS: &[(&str, usize, &str)] = &[
    (
        "/\u{5}DocumentSummaryInformation",
        188,
        "8ba4b2becd35181ae0fb783797f4d26a7fa27aa1183ec8f352a579e4769991c0",
    ),
    (
        "/\u{5}SummaryInformation",
        17_176,
        "b7243c446b7d72f44568d5859d215d8a7396178f7acb76961cfe9da84fc5b27d",
    ),
    (
        "/Dgn-Md/#000000/Dgn^CA/^AH",
        4,
        "df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119",
    ),
    (
        "/Dgn-Md/#000000/Dgn^G/$1",
        1_496,
        "44e0a2d8af5ca39741a196dee41566a9f2c1e729cb5ecf92ed5902bcb2fedeb0",
    ),
    (
        "/Dgn-Md/#000000/Dgn^GA/$1",
        16,
        "992c30aaa46eff451038a19d0e1c6d6d1f41e27453adf66bfd84057dca8c6051",
    ),
    (
        "/Dgn-Md/#000000/Dgn^GA/^AH",
        4,
        "67abdd721024f0ff4e0b3f4c2fc13bc5bad42d0b7851d456d88d203d15aaa450",
    ),
    (
        "/Dgn-Md/#000000/Dgn~Mh",
        208,
        "5ab72e375faae7c57137ccd383720744be9ec494d7b6598d8ea68f26a29f4a8d",
    ),
    (
        "/Dgn^Ix/Dgn~Mix",
        69,
        "03a84563a58901ffc2df7a6fae7aa9ba07684d7e88df2b24838cd0be9438a649",
    ),
    (
        "/Dgn^Nm/$1",
        1_912,
        "c0ffb38925a008b382abca344912c2ad718bf7e51b4cfcc287e53dd73bd67621",
    ),
    (
        "/Dgn^NmA/$1",
        152,
        "eb2b206a470fc4dd4aec83535776edcef013ab1d529b9409dac3a86d70f476b0",
    ),
    (
        "/Dgn^NmA/^AH",
        4,
        "67abdd721024f0ff4e0b3f4c2fc13bc5bad42d0b7851d456d88d203d15aaa450",
    ),
    (
        "/Dgn~H",
        68,
        "3c95ea79c7a5e83d72a6b5814548164ebb0086440a5f5749ed8743d64592baa0",
    ),
    (
        "/Dgn~Mf",
        14,
        "723afff798e9ac3d5aa2e04c3a2ecf662bf8b1c7c7d30927452e42a4955e80ec",
    ),
    (
        "/Dgn~S",
        208,
        "86ff85227f7cc42cc3293ebdeeb10d8660b51eb429dd1d6c999ddf14cb59db73",
    ),
    (
        "/Oda~SH",
        16,
        "f859b1e4654ed5bce40999c554ce596829aeac9cc593b98a6fa0f8a91b22bd01",
    ),
];

#[test]
fn extracts_real_v8_fixture_with_stable_stream_inventory() {
    let stream_set = read_v8_streams(V8, V8ReadOptions::default()).unwrap();

    assert_eq!(stream_set.container.cfb_version, 3);
    assert_eq!(stream_set.container.entries.len(), 24);
    assert_eq!(
        stream_set.container.model_storage_paths,
        ["/Dgn-Md/#000000"]
    );
    assert_eq!(stream_set.streams.len(), STREAMS.len());
    assert_eq!(stream_set.total_size(), 21_535);

    for &(path, size, expected_sha256) in STREAMS {
        let stream = stream_set
            .get(path)
            .unwrap_or_else(|| panic!("missing {path}"));
        assert_eq!(stream.len(), size, "size mismatch for {path}");
        assert_eq!(sha256_hex(stream.as_bytes()), expected_sha256, "{path}");
    }
}

#[test]
fn reads_one_real_stream_and_enforces_total_limit_before_extraction() {
    let header = read_v8_stream(V8, "/Dgn~H", V8ReadOptions::default()).unwrap();
    assert_eq!(header.len(), 68);

    let options = V8ReadOptions {
        max_total_stream_bytes: 21_534,
        ..V8ReadOptions::default()
    };
    assert!(matches!(
        read_v8_streams(V8, options),
        Err(DgnError::CfbTotalStreamSizeLimitExceeded {
            size: 21_535,
            limit: 21_534,
        })
    ));
}

#[test]
fn scans_real_model_index_graphics_names_and_auxiliary_records() {
    let raw = scan_v8_objects(V8, V8ScanOptions::default()).unwrap();

    assert_eq!(raw.models.len(), 1);
    let model = &raw.models[0];
    assert_eq!(model.index.storage_index, 0);
    assert_eq!(model.index.model_number, 1);
    assert_eq!(model.index.name, "my_model");
    assert_eq!(model.index.description, "my_description");
    assert_eq!(model.storage_path, "/Dgn-Md/#000000");
    assert_eq!(model.graphical_pages.len(), 1);
    assert_eq!(model.graphical_pages[0].header.record_count, 58);
    assert_eq!(model.graphical_pages[0].inflated_size, 10_868);
    assert_eq!(model.graphical_objects().count(), 58);
    assert_eq!(model.graphical_auxiliary_pages.len(), 1);
    assert!(model.graphical_auxiliary_pages[0].records.is_empty());

    let first = model.graphical_objects().next().unwrap();
    assert_eq!(first.element_type, 3);
    assert_eq!(first.role, V8ObjectRole::Standalone);
    assert_eq!(first.level, Some(1));
    assert_eq!(first.element_id, Some(36));
    assert_eq!(first.model_id, Some(model.index.model_id));
    assert_eq!(first.len(), 152);
    assert!(first.attribute_bytes().is_empty());

    assert_eq!(raw.named_pages.len(), 1);
    assert_eq!(raw.named_pages[0].objects.len(), 25);
    assert_eq!(raw.named_auxiliary_pages.len(), 1);
    assert_eq!(raw.named_auxiliary_pages[0].records.len(), 1);
    assert_eq!(raw.graphical_object_count(), 58);
    assert_eq!(raw.total_object_count(), 83);
    assert_eq!(raw.total_inflated_bytes, 31_942);
}

#[test]
fn decodes_real_model_metadata_primitives_text_and_hierarchy() {
    let document = read_v8(V8, V8ScanOptions::default()).unwrap();
    assert_eq!(document.models.len(), 1);
    let model = &document.models[0];
    assert_eq!(model.metadata.name, "my_model");
    assert_eq!(model.metadata.description, "my_description");
    assert_eq!(model.metadata.dimension, V8Dimension::Three);
    assert_eq!(model.metadata.uor_per_master, 10_000.0);
    assert_eq!(model.metadata.master_unit.as_deref(), Some("m"));
    assert_eq!(model.metadata.sub_unit.as_deref(), Some("mm"));
    assert_eq!(model.metadata.extents_uor.low, [-19_577, -24_358, 0]);
    assert_eq!(model.metadata.extents_uor.high, [60_000, 70_000, 80_000]);

    assert_eq!(model.elements.len(), 58);
    assert_eq!(model.entities().count(), 34);
    assert_eq!(
        model
            .entities()
            .map(|element| element.raw.element_type)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([2, 3, 4, 6, 11, 12, 14, 15, 16, 17, 22, 27, 35, 36])
    );
    assert_eq!(
        model
            .entities()
            .filter(|element| element.common.dimension == V8Dimension::Two)
            .count(),
        19
    );
    assert_eq!(
        model
            .entities()
            .filter(|element| element.common.dimension == V8Dimension::Three)
            .count(),
        15
    );
    let V8ElementData::Line { start, end } = &model.elements[0].data else {
        panic!("first object is not a line")
    };
    assert_eq!(start.uor.as_array(), [0.0, 10_000.0, 20_000.0]);
    assert_eq!(end.uor.as_array(), [0.0, 10_000.0, 20_000.0]);
    assert_eq!(start.master.as_array(), [0.0, 1.0, 2.0]);
    assert_eq!(model.elements[0].common.level, 1);
    assert_eq!(model.elements[0].common.graphic_group, 2);
    assert_eq!(model.elements[0].common.color_index, 3);
    assert_eq!(model.elements[0].common.line_weight, 5);
    assert_eq!(model.elements[0].common.line_style, 4);

    let V8ElementData::Text {
        text,
        text_bytes,
        origin,
        ..
    } = &model.elements[4].data
    else {
        panic!("fifth object is not text")
    };
    assert_eq!(text, "myTéxt");
    assert_eq!(text_bytes.as_ref(), b"\xff\xfe\x01\0myT\xe9xt");
    assert_eq!(origin.master.as_array(), [0.0, 1.0, 0.0]);

    assert_eq!(model.elements[6].child_indices, [7]);
    assert_eq!(model.elements[7].parent_index, Some(6));
    assert_eq!(model.elements[23].child_indices, [24]);
    assert_eq!(model.elements[27].child_indices, [28, 29]);
    assert_eq!(model.elements[38].child_indices, [39, 40]);
    assert_eq!(model.elements[44].child_indices, [45, 46, 47]);

    let V8ElementData::SharedCellInstance { name, origin, .. } = &model.elements[56].data else {
        panic!("type 35 is not a shared-cell instance")
    };
    assert_eq!(name.as_deref(), Some("Named definition"));
    assert_eq!(origin.master.as_array(), [0.0, 1.0, 2.0]);
    assert!(matches!(model.elements[57].data, V8ElementData::Unknown));
}

#[test]
fn enforces_v8_semantic_resource_limits() {
    assert!(matches!(
        read_v8(
            V8,
            V8ScanOptions {
                max_vertices: 0,
                ..V8ScanOptions::default()
            }
        ),
        Err(DgnError::V8VertexLimitExceeded { limit: 0, .. })
    ));
    assert!(matches!(
        read_v8(
            V8,
            V8ScanOptions {
                max_hierarchy_depth: 0,
                ..V8ScanOptions::default()
            }
        ),
        Err(DgnError::V8HierarchyDepthLimitExceeded { limit: 0, .. })
    ));
    assert!(matches!(
        scan_v8_objects(
            V8,
            V8ScanOptions {
                max_total_inflated_bytes: 31_941,
                ..V8ScanOptions::default()
            }
        ),
        Err(DgnError::V8TotalInflatedSizeLimitExceeded { limit: 31_941 })
    ));
    assert!(matches!(
        read_v8(
            V8,
            V8ScanOptions {
                max_string_bytes: 1,
                ..V8ScanOptions::default()
            }
        ),
        Err(DgnError::V8StringLimitExceeded { limit: 1, .. })
    ));
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
