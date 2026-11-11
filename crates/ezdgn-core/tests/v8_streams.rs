use std::collections::BTreeSet;

use ezdgn_core::{
    decode_v8_item_instance, read_v8, read_v8_stream, read_v8_streams, scan_v8_objects, DgnError,
    V8Dimension, V8ElementData, V8ObjectRole, V8Point3, V8ReadOptions, V8ScanOptions, V8TagValue,
};
use sha2::{Digest, Sha256};

const V8: &[u8] = include_bytes!("../../../tests/data/dgn/v8/test_dgnv8.dgn");
/// Written by the test builder of this repository from made-up values.
const CONSTRUCTED_TAGS: &[u8] = include_bytes!("../../../tests/data/dgn/v8/constructed_tags.dgn");

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
    // The one auxiliary record of the fixture holds one entry: an 8-byte
    // prefix (3, inflated size 192) and a zlib stream. The entry count is
    // the header word exposed as `flags`.
    let record = &raw.named_auxiliary_pages[0].records[0];
    assert_eq!(
        (record.kind, record.flags, record.payload().len()),
        (170, 1, 122)
    );
    let entries = record.entries().unwrap();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!((entry.index, entry.offset), (0, 0));
    assert_eq!(
        (entry.type_id, entry.type_major(), entry.type_minor()),
        (0x5709_0000, 0x5709, 0)
    );
    assert_eq!(
        (entry.entry_id, entry.size_word, entry.reserved),
        (0, 102, 0)
    );
    assert!(!entry.size_flag());
    assert_eq!(entry.data().len(), 102);
    assert_eq!(entry.data()[..10], [3, 0, 0, 0, 0xc0, 0, 0, 0, 0x78, 0x9c]);
    assert_eq!(record.payload()[118..], [1, 0, 0, 0]);
    assert!(decode_v8_item_instance(entry).is_none());
    assert_eq!(raw.graphical_object_count(), 58);
    assert_eq!(raw.total_object_count(), 83);
    assert_eq!(raw.total_inflated_bytes, 31_942);
}

#[test]
fn the_fixture_shared_cell_instance_finds_and_places_its_definition() {
    let document = read_v8(V8, V8ScanOptions::default()).unwrap();
    let definitions = document
        .shared_cell_definitions(0, V8ScanOptions::default())
        .unwrap();
    assert_eq!(definitions.len(), 1);
    let definition = &definitions[0];
    assert_eq!(definition.name, "Named definition");
    assert_eq!(
        definition
            .elements
            .iter()
            .map(|element| (element.kind(), element.parent_index))
            .collect::<Vec<_>>(),
        [("SHARED_CELL_DEFINITION", None), ("ELLIPSE", Some(0))]
    );
    // a circle of radius 1 UOR at the definition origin
    let V8ElementData::Ellipse {
        center,
        primary_axis_uor,
        secondary_axis_uor,
        primary_axis_master,
        ..
    } = &definition.elements[1].data
    else {
        panic!("expected an ellipse");
    };
    assert_eq!((center.uor.x, center.uor.y, center.uor.z), (0.0, 0.0, 0.0));
    assert_eq!((*primary_axis_uor, *secondary_axis_uor), (1.0, 1.0));
    assert_eq!(*primary_axis_master, 0.0001);

    let instances = document.models[0]
        .elements
        .iter()
        .filter(|element| element.kind() == "SHARED_CELL_INSTANCE")
        .collect::<Vec<_>>();
    assert_eq!(instances.len(), 1);
    let instance = instances[0];
    assert!(definition.matches(&instance.data));
    let V8ElementData::SharedCellInstance { transform, .. } = &instance.data else {
        unreachable!();
    };
    // The instance scales by 10,000. origin + M * p, applied to the two
    // corners of the circle's box, gives the range the instance stores.
    // This matrix is diagonal, so it does not show the order of the nine
    // values; a constructed test covers that.
    assert_eq!(
        transform[6..],
        [1e4, 0.0, 0.0, 0.0, 1e4, 0.0, 0.0, 0.0, 1e4]
    );
    let place = |x: f64, y: f64| {
        let point = instance
            .data
            .place_definition_point(V8Point3::new(x, y, 0.0))
            .unwrap();
        (point.x, point.y)
    };
    let range = instance.common.range_uor;
    assert_eq!(place(-1.0, -1.0), (-1e4, 0.0));
    assert_eq!(place(1.0, 1.0), (1e4, 2e4));
    assert_eq!(
        place(-1.0, -1.0),
        (range.low[0] as f64, range.low[1] as f64)
    );
    assert_eq!(
        place(1.0, 1.0),
        (range.high[0] as f64, range.high[1] as f64)
    );
    assert_eq!(
        document
            .shared_cell_definitions(1, V8ScanOptions::default())
            .unwrap_err(),
        DgnError::V8ModelIndexOutOfRange { index: 1, count: 1 }
    );
}

#[test]
fn the_fixture_holds_no_tag_set_and_its_one_3d_type_37_object_stays_unknown() {
    let document = read_v8(V8, V8ScanOptions::default()).unwrap();
    assert!(document.tag_sets().is_empty());
    assert_eq!(document.raw.tag_set_objects().count(), 0);
    let objects = document.models[0]
        .elements
        .iter()
        .filter(|element| element.raw.element_type == 37)
        .collect::<Vec<_>>();
    assert_eq!(objects.len(), 1);
    // Tag elements are read in 2D only. This object is stored as 3D, names
    // no tag set and no host, and holds tag ID 0.
    let object = objects[0];
    assert_eq!(object.common.stored_dimension, V8Dimension::Three);
    assert_eq!((object.kind(), object.linkages.len()), ("UNKNOWN", 0));
    assert_eq!(object.raw.primary_bytes()[0xd0..0xd4], [0, 0, 3, 0]);
}

#[test]
fn stored_3d_texts_ellipses_and_arcs_expose_a_quaternion() {
    let document = read_v8(V8, V8ScanOptions::default()).unwrap();
    let mut texts = Vec::new();
    let mut curves = 0;
    for element in &document.models[0].elements {
        let stored_3d = element.common.stored_dimension == V8Dimension::Three;
        match &element.data {
            V8ElementData::Text { orientation, .. } => {
                assert_eq!(element.data.quaternion().is_some(), stored_3d);
                if stored_3d {
                    assert_eq!(orientation.len(), 6);
                    texts.push(element.data.quaternion().unwrap());
                }
            }
            V8ElementData::Ellipse { orientation, .. } | V8ElementData::Arc { orientation, .. } => {
                assert_eq!(element.data.quaternion().is_some(), stored_3d);
                if stored_3d {
                    curves += 1;
                    assert_eq!(element.data.quaternion().unwrap()[..], orientation[..]);
                }
            }
            _ => assert_eq!(element.data.quaternion(), None),
        }
    }
    // the fixture's two 3D texts are not turned: the scalar part comes first
    assert_eq!(texts, [[1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]);
    assert!(curves > 0);
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
        width_multiplier_raw,
        height_multiplier_raw,
        width_uor,
        height_uor,
        width_master,
        height_master,
        ..
    } = &model.elements[4].data
    else {
        panic!("fifth object is not text")
    };
    assert_eq!(text, "myTéxt");
    assert_eq!(text_bytes.as_ref(), b"\xff\xfe\x01\0myT\xe9xt");
    assert_eq!(origin.master.as_array(), [0.0, 1.0, 0.0]);
    assert!((*width_multiplier_raw - 1_666_666.666_666_666_5).abs() < 1e-9);
    assert!((*height_multiplier_raw - 1_666_666.666_666_666_5).abs() < 1e-9);
    assert!((*width_uor - 10_000.0).abs() < 1e-9);
    assert!((*height_uor - 10_000.0).abs() < 1e-9);
    assert!((*width_master - 1.0).abs() < 1e-12);
    assert!((*height_master - 1.0).abs() < 1e-12);

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

#[test]
fn the_constructed_tag_file_holds_the_tags_its_builder_wrote() {
    let document = read_v8(CONSTRUCTED_TAGS, V8ScanOptions::default()).unwrap();
    let elements = &document.models[0].elements;
    assert_eq!(
        elements
            .iter()
            .map(|element| element.kind())
            .collect::<Vec<_>>(),
        [
            "SHARED_CELL_INSTANCE",
            "TAG",
            "TAG",
            "TAG",
            "UNKNOWN",
            "UNKNOWN_COMPLEX",
            "UNKNOWN"
        ]
    );
    let V8ElementData::Tag {
        tag_id,
        value,
        tag_set_element_id,
        host_element_id,
        origin,
        ..
    } = &elements[1].data
    else {
        panic!("expected a tag");
    };
    assert_eq!(*tag_id, 1);
    assert_eq!(*value, Some(V8TagValue::Text("Oak".to_owned())));
    assert_eq!(*host_element_id, Some(elements[0].common.element_id));
    let origin = origin.as_ref().unwrap();
    assert_eq!((origin.uor.x, origin.uor.y), (960.0, -100.0));
    let sets = document.tag_sets();
    assert_eq!(sets.len(), 1);
    assert_eq!(Some(sets[0].element_id), *tag_set_element_id);
    assert_eq!(sets[0].definition(1).unwrap().name, "Variety");
}
