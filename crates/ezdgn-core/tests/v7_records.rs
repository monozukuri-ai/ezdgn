use ezdgn_core::{
    decode_common_header, decode_design_settings, detect_format, inspect_v8_container, read_v7_2d,
    scan_records, write_v7_2d, DgnError, DgnFormat, ElementData2D, Point2, RawPoint,
    RecordStreamEnd, ScanOptions, V7Dimension, V7ElementStyle, V7WriteOptions, V8CfbEntryKind,
    WritableElement2D, DEFAULT_MAX_CFB_ENTRIES,
};

const SMALLTEST: &[u8] = include_bytes!("../../../tests/data/dgn/v7/smalltest.dgn");
const SEED_2D: &[u8] = include_bytes!("../../../tests/data/dgn/v7/seed_2d.dgn");
const SEED_3D: &[u8] = include_bytes!("../../../tests/data/dgn/v7/seed_3d.dgn");
const KNOT_OOB: &[u8] = include_bytes!("../../../tests/data/dgn/malformed/knot_oob.dgn");
const V8: &[u8] = include_bytes!("../../../tests/data/dgn/v8/test_dgnv8.dgn");

#[test]
fn scans_smalltest_exact_record_boundaries() {
    let scan = scan_records(SMALLTEST, ScanOptions::default()).unwrap();
    assert_eq!(scan.format, DgnFormat::V7(V7Dimension::Two));
    assert_eq!(scan.source_size, 10_752);
    assert_eq!(scan.records.len(), 15);
    assert_eq!(
        scan.termination,
        RecordStreamEnd::EndMarker {
            offset: 10_424,
            trailing_bytes: 326,
        }
    );

    let offsets = scan
        .records
        .iter()
        .map(|record| record.offset)
        .collect::<Vec<_>>();
    assert_eq!(
        offsets,
        [
            0, 1536, 1892, 2048, 3584, 3812, 4416, 5616, 6816, 7216, 8616, 10136, 10206, 10278,
            10372,
        ]
    );
    let element_types = scan
        .records
        .iter()
        .map(|record| record.header.element_type)
        .collect::<Vec<_>>();
    assert_eq!(
        element_types,
        [9, 8, 10, 9, 5, 66, 66, 66, 66, 66, 66, 17, 15, 6, 3]
    );
    assert_eq!(scan.records[11].header.level, 1);
    assert_eq!(scan.records[11].bytes.len(), 70);
    assert_eq!(&SMALLTEST[10_136..10_206], scan.records[11].bytes);
}

#[test]
fn handles_both_valid_v7_termination_forms() {
    let seed_2d = scan_records(SEED_2D, ScanOptions::default()).unwrap();
    assert_eq!(seed_2d.records.len(), 12);
    assert_eq!(
        seed_2d.termination,
        RecordStreamEnd::EndMarker {
            offset: 9130,
            trailing_bytes: 84,
        }
    );

    let seed_3d = scan_records(SEED_3D, ScanOptions::default()).unwrap();
    assert_eq!(seed_3d.format, DgnFormat::V7(V7Dimension::Three));
    assert_eq!(seed_3d.records.len(), 3);
    assert_eq!(
        seed_3d.termination,
        RecordStreamEnd::PhysicalEof { offset: 2048 }
    );
}

#[test]
fn keeps_semantically_malformed_knot_as_a_bounded_raw_record() {
    let scan = scan_records(KNOT_OOB, ScanOptions::default()).unwrap();
    assert_eq!(scan.records.len(), 2);
    assert_eq!(scan.records[1].header.element_type, 26);
    assert_eq!(scan.records[1].offset, 1536);
    assert_eq!(scan.records[1].bytes.len(), 40);
    assert_eq!(
        scan.termination,
        RecordStreamEnd::EndMarker {
            offset: 1576,
            trailing_bytes: 0,
        }
    );
}

#[test]
fn detects_but_does_not_scan_v8_container() {
    assert_eq!(detect_format(V8), Ok(DgnFormat::V8Cfb));
    let info = inspect_v8_container(V8, DEFAULT_MAX_CFB_ENTRIES).unwrap();
    assert_eq!(info.cfb_version, 3);
    assert!(info.has_dgn_v8_markers);
    assert!(info.missing_markers.is_empty());
    assert_eq!(info.model_storage_paths, ["/Dgn-Md/#000000"]);
    assert!(info.entries.iter().any(|entry| {
        entry.path == "/Dgn~H"
            && entry.kind == V8CfbEntryKind::Stream
            && entry.size_bytes == Some(68)
    }));
    assert!(matches!(
        scan_records(V8, ScanOptions::default()),
        Err(DgnError::UnsupportedFormat {
            format: DgnFormat::V8Cfb
        })
    ));
}

#[test]
fn every_smalltest_prefix_is_handled_without_a_panic() {
    for end in 0..=SMALLTEST.len() {
        let _ = scan_records(&SMALLTEST[..end], ScanOptions::default());
    }
}

#[test]
fn every_decodable_smalltest_prefix_is_safe_for_phase_two() {
    for end in 0..=SMALLTEST.len() {
        let Ok(scan) = scan_records(&SMALLTEST[..end], ScanOptions::default()) else {
            continue;
        };
        let Ok(settings) = decode_design_settings(&scan) else {
            continue;
        };
        for record in scan.records.iter().copied() {
            let _ = decode_common_header(record, settings.dimension);
        }
    }
}

#[test]
fn decodes_design_settings_from_real_tcb_records() {
    let smalltest_scan = scan_records(SMALLTEST, ScanOptions::default()).unwrap();
    let smalltest = decode_design_settings(&smalltest_scan).unwrap();
    assert_eq!(smalltest.dimension, V7Dimension::Two);
    assert_eq!(smalltest.subunits_per_master, 10);
    assert_eq!(smalltest.uor_per_subunit, 1000);
    assert_eq!(smalltest.uor_per_master(), 10_000);
    assert_eq!(smalltest.master_unit_label, *b"mu");
    assert_eq!(smalltest.sub_unit_label, *b"su");
    assert_eq!(smalltest.global_origin_uor, [0.0; 3]);
    assert_eq!(smalltest.scale(), Some(0.0001));

    let seed_2d_scan = scan_records(SEED_2D, ScanOptions::default()).unwrap();
    let seed_2d = decode_design_settings(&seed_2d_scan).unwrap();
    assert_eq!(seed_2d.subunits_per_master, 10);
    assert_eq!(seed_2d.uor_per_subunit, 12);
    assert_eq!(seed_2d.master_unit_label, *b"ft");
    assert_eq!(seed_2d.sub_unit_label, *b"tf");
    assert_eq!(
        seed_2d.global_origin_uor,
        [-249_879_416.0, -669_487_710.0, 0.0]
    );
    let seed_2d_origin = seed_2d.global_origin_master().unwrap();
    assert_close(seed_2d_origin[0], -2_082_328.466_666_666_6);
    assert_close(seed_2d_origin[1], -5_579_064.25);

    let seed_3d_scan = scan_records(SEED_3D, ScanOptions::default()).unwrap();
    let seed_3d = decode_design_settings(&seed_3d_scan).unwrap();
    assert_eq!(seed_3d.dimension, V7Dimension::Three);
    assert_eq!(seed_3d.subunits_per_master, 1000);
    assert_eq!(seed_3d.uor_per_subunit, 1);
    assert_eq!(seed_3d.master_unit_label, [b'm', 0]);
    assert_eq!(seed_3d.sub_unit_label, *b"mm");
    assert_eq!(seed_3d.global_origin_uor, [0.0; 3]);

    let knot_scan = scan_records(KNOT_OOB, ScanOptions::default()).unwrap();
    let knot = decode_design_settings(&knot_scan).unwrap();
    assert_eq!(knot.uor_per_master(), 0);
    assert_eq!(knot.scale(), None);
    assert_eq!(knot.global_origin_master(), None);
}

#[test]
fn decodes_common_ranges_properties_and_symbology() {
    let scan = scan_records(SMALLTEST, ScanOptions::default()).unwrap();
    let settings = decode_design_settings(&scan).unwrap();

    assert_eq!(
        decode_common_header(scan.records[0], settings.dimension).unwrap(),
        None
    );
    assert_eq!(
        decode_common_header(scan.records[2], settings.dimension).unwrap(),
        None
    );

    let text = decode_common_header(scan.records[11], settings.dimension)
        .unwrap()
        .unwrap();
    assert_eq!(
        text.range.low,
        RawPoint {
            x: 7365,
            y: 37_198,
            z: None,
        }
    );
    assert_eq!(
        text.range.high,
        RawPoint {
            x: 94_083,
            y: 57_198,
            z: None,
        }
    );
    assert_eq!(text.graphic_group, 0);
    assert_eq!(text.attribute_index, 19);
    assert!(text.properties.new);
    assert!(!text.properties.has_attributes);
    assert!(text.properties.is_snappable());
    assert!(text.properties.is_planar());
    assert_eq!(text.symbology.style, 0);
    assert_eq!(text.symbology.weight, 0);
    assert_eq!(text.symbology.color, 0);
    assert_eq!(text.attribute_offset, None);

    let text_master = text.range.to_master(settings).unwrap();
    assert_close(text_master.low.x, 0.7365);
    assert_close(text_master.low.y, 3.7198);
    assert_eq!(text_master.low.z, None);
    assert_close(text_master.high.x, 9.4083);
    assert_close(text_master.high.y, 5.7198);
    assert_eq!(
        text.range.to_master(ezdgn_core::DesignSettings {
            dimension: V7Dimension::Three,
            ..settings
        }),
        None
    );

    let ellipse = decode_common_header(scan.records[12], settings.dimension)
        .unwrap()
        .unwrap();
    let ellipse_master = ellipse.range.to_master(settings).unwrap();
    assert_close(ellipse_master.low.x, 0.3285);
    assert_close(ellipse_master.low.y, -0.0961);
    assert_close(ellipse_master.high.x, 9.6878);
    assert_close(ellipse_master.high.y, 9.2631);

    let shape = decode_common_header(scan.records[13], settings.dimension)
        .unwrap()
        .unwrap();
    assert_eq!(shape.properties.raw, 0x0e00);
    assert!(shape.properties.new);
    assert!(shape.properties.modified);
    assert!(shape.properties.has_attributes);
    assert_eq!(shape.symbology.raw, 0x5300);
    assert_eq!(shape.symbology.color, 83);
    assert_eq!(shape.attribute_offset, Some(78));
    assert_eq!(shape.attribute_length, 16);
}

#[test]
fn rejects_phase_two_header_inconsistencies_without_panicking() {
    let mut dimension_mismatch = SMALLTEST.to_vec();
    dimension_mismatch[1214] |= 0x40;
    let mismatch_scan = scan_records(&dimension_mismatch, ScanOptions::default()).unwrap();
    assert!(matches!(
        decode_design_settings(&mismatch_scan),
        Err(DgnError::DimensionMismatch {
            signature: V7Dimension::Two,
            tcb: V7Dimension::Three,
        })
    ));

    let mut invalid_attribute = SMALLTEST.to_vec();
    invalid_attribute[10_278 + 30] = 0xff;
    invalid_attribute[10_278 + 31] = 0xff;
    let invalid_scan = scan_records(&invalid_attribute, ScanOptions::default()).unwrap();
    assert!(matches!(
        decode_common_header(invalid_scan.records[13], V7Dimension::Two),
        Err(DgnError::InvalidAttributeOffset {
            offset: 10_278,
            element_type: 6,
            attribute_offset: 131_102,
            record_size: 94,
        })
    ));
}

#[test]
fn decodes_smalltest_phase_three_primitives_exactly() {
    let document = read_v7_2d(SMALLTEST, ScanOptions::default()).unwrap();
    assert_eq!(document.elements.len(), 15);
    assert_eq!(document.active_color_table, None);
    assert_eq!(
        document
            .elements
            .iter()
            .filter(|element| element.data.is_graphic())
            .map(|element| element.data.kind())
            .collect::<Vec<_>>(),
        ["TEXT", "ELLIPSE", "SHAPE", "LINE"]
    );

    let ElementData2D::Text(text) = &document.elements[11].data else {
        panic!("expected text");
    };
    assert_eq!(text.font_id, 3);
    assert_eq!(text.justification, 7);
    assert_eq!(text.length_multiplier_raw, 1_666_667);
    assert_eq!(text.height_multiplier_raw, 1_666_667);
    assert_close(text.length_multiplier_master.unwrap(), 1.000_000_2);
    assert_close(text.height_multiplier_master.unwrap(), 1.000_000_2);
    assert_eq!(text.rotation_raw, 0);
    assert_eq!(text.rotation_degrees, 0.0);
    assert_eq!(
        text.origin_uor,
        Point2 {
            x: 7_365,
            y: 42_198
        }
    );
    assert_eq!(text.text_offset, 60);
    assert_eq!(text.text_bytes, b"Demo Text");
    let text_origin = text.origin_master.unwrap();
    assert_close(text_origin.x, 0.7365);
    assert_close(text_origin.y, 4.2198);

    let ElementData2D::Ellipse(ellipse) = &document.elements[12].data else {
        panic!("expected ellipse");
    };
    assert_close(ellipse.primary_axis_uor, 46_796.065_838_914_28);
    assert_close(ellipse.secondary_axis_uor, 46_796.065_838_914_28);
    assert_close(ellipse.primary_axis_master.unwrap(), 4.679_606_583_891_428);
    assert_close(ellipse.center_uor.x, 50_082.000_000_000_01);
    assert_close(ellipse.center_uor.y, 45_835.0);
    let ellipse_center = ellipse.center_master.unwrap();
    assert_close(ellipse_center.x, 5.0082);
    assert_close(ellipse_center.y, 4.5835);
    assert_eq!(ellipse.rotation_degrees, 0.0);

    let ElementData2D::Shape(shape) = &document.elements[13].data else {
        panic!("expected shape");
    };
    assert_eq!(
        shape.vertices_uor,
        [
            Point2 {
                x: 45_355,
                y: 33_170,
            },
            Point2 {
                x: 43_832,
                y: 26_517,
            },
            Point2 {
                x: 49_441,
                y: 25_235,
            },
            Point2 {
                x: 48_320,
                y: 33_331,
            },
            Point2 {
                x: 45_355,
                y: 33_170,
            },
        ]
    );
    assert_eq!(shape.vertices_uor.first(), shape.vertices_uor.last());
    let shape_master = shape.vertices_master.as_ref().unwrap();
    assert_close(shape_master[2].x, 4.9441);
    assert_close(shape_master[2].y, 2.5235);

    let ElementData2D::Line(line) = &document.elements[14].data else {
        panic!("expected line");
    };
    assert_eq!(
        line.start_uor,
        Point2 {
            x: 25_562,
            y: 57_218,
        }
    );
    assert_eq!(
        line.end_uor,
        Point2 {
            x: 25_242,
            y: 60_709,
        }
    );
    assert_close(line.start_master.unwrap().x, 2.5562);
    assert_close(line.end_master.unwrap().y, 6.0709);

    for (element, raw) in document.elements.iter().zip(document.scan.records.iter()) {
        assert_eq!(element.raw.bytes, raw.bytes);
    }
}

#[test]
fn phase_three_rejects_3d_but_keeps_zero_scale_unknown_records() {
    assert!(matches!(
        read_v7_2d(SEED_3D, ScanOptions::default()),
        Err(DgnError::UnsupportedDimension {
            dimension: V7Dimension::Three,
        })
    ));

    let knot = read_v7_2d(KNOT_OOB, ScanOptions::default()).unwrap();
    assert_eq!(knot.elements.len(), 2);
    let ElementData2D::BSplineKnot(values) = &knot.elements[1].data else {
        panic!("expected a bounded B-spline knot array");
    };
    assert_eq!(values.values_raw, [0]);
    assert_eq!(values.values, [0.0]);
    assert_eq!(knot.elements[1].raw.bytes, &KNOT_OOB[1536..1576]);
}

#[test]
fn restores_nested_phase_four_hierarchy_from_declared_word_ranges() {
    let line = synthetic_record(3, 2, &[0; 16], true, &[]);
    let mut curve_body = vec![0; 18];
    curve_body[..2].copy_from_slice(&2_u16.to_le_bytes());
    put_middle_i32(&mut curve_body, 2, 10);
    put_middle_i32(&mut curve_body, 6, 20);
    put_middle_i32(&mut curve_body, 10, 30);
    put_middle_i32(&mut curve_body, 14, 40);
    let curve = synthetic_record(11, 2, &curve_body, true, &[]);

    let complex_size = 48 + line.len() + curve.len();
    let mut complex_body = vec![0; 4];
    complex_body[..2].copy_from_slice(
        &u16::try_from((complex_size - 38) / 2)
            .unwrap()
            .to_le_bytes(),
    );
    complex_body[2..4].copy_from_slice(&2_u16.to_le_bytes());
    let complex = synthetic_record(14, 2, &complex_body, true, &[0; 8]);

    let cell_size = 92 + complex_size;
    let mut cell_body = vec![0; 56];
    cell_body[..2].copy_from_slice(&u16::try_from((cell_size - 38) / 2).unwrap().to_le_bytes());
    let cell = synthetic_record(2, 2, &cell_body, false, &[]);
    let data = with_synthetic_records(&[cell, complex, line, curve]);
    let document = read_v7_2d(&data, ScanOptions::default()).unwrap();

    let cell_index = 11;
    let complex_index = 12;
    assert!(matches!(
        document.elements[cell_index].data,
        ElementData2D::Cell(_)
    ));
    assert_eq!(document.elements[cell_index].parent_index, None);
    assert_eq!(document.elements[cell_index].child_indices, [complex_index]);
    assert!(matches!(
        document.elements[complex_index].data,
        ElementData2D::ComplexShape(_)
    ));
    assert_eq!(document.elements[complex_index].child_indices, [13, 14]);
    assert_eq!(document.elements[13].parent_index, Some(complex_index));
    assert!(matches!(
        document.elements[14].data,
        ElementData2D::Curve(_)
    ));
    assert_eq!(
        document.root_indices(),
        (0..=cell_index).collect::<Vec<_>>()
    );
}

#[test]
fn rejects_phase_four_component_count_mismatches() {
    let line = synthetic_record(3, 2, &[0; 16], true, &[]);
    let group_size = 48 + line.len();
    let mut header_body = vec![0; 4];
    header_body[..2].copy_from_slice(&u16::try_from((group_size - 38) / 2).unwrap().to_le_bytes());
    header_body[2..4].copy_from_slice(&2_u16.to_le_bytes());
    let header = synthetic_record(12, 2, &header_body, false, &[0; 8]);
    let data = with_synthetic_records(&[header, line]);
    assert!(matches!(
        read_v7_2d(&data, ScanOptions::default()),
        Err(DgnError::ComplexElementCountMismatch {
            declared: 2,
            actual: 1,
            ..
        })
    ));
}

#[test]
fn every_scannable_smalltest_prefix_is_safe_for_phase_three() {
    for end in 0..=SMALLTEST.len() {
        let _ = read_v7_2d(&SMALLTEST[..end], ScanOptions::default());
    }
}

#[test]
fn phase_five_writes_native_primitives_from_a_2d_seed() {
    let styled = V7ElementStyle {
        level: 7,
        color: 12,
        line_style: 3,
        line_weight: 5,
        graphic_group: 42,
        properties: 0x0200,
    };
    let elements = vec![
        WritableElement2D::Line {
            start: Point2 { x: 1.25, y: -2.5 },
            end: Point2 { x: 4.5, y: 3.75 },
            style: styled,
        },
        WritableElement2D::LineString {
            vertices: vec![
                Point2 { x: 0.0, y: 0.0 },
                Point2 { x: 1.0, y: 2.0 },
                Point2 { x: 3.0, y: 1.0 },
            ],
            style: V7ElementStyle::default(),
        },
        WritableElement2D::Shape {
            vertices: vec![
                Point2 { x: 0.0, y: 0.0 },
                Point2 { x: 2.0, y: 0.0 },
                Point2 { x: 2.0, y: 2.0 },
                Point2 { x: 0.0, y: 0.0 },
            ],
            fill_color: Some(83),
            style: V7ElementStyle::default(),
        },
        WritableElement2D::Curve {
            vertices: vec![
                Point2 { x: -1.0, y: 0.0 },
                Point2 { x: 0.0, y: 1.0 },
                Point2 { x: 1.0, y: 0.0 },
            ],
            style: V7ElementStyle::default(),
        },
        WritableElement2D::Ellipse {
            center: Point2 { x: 10.0, y: 20.0 },
            primary_axis: 4.0,
            secondary_axis: 2.0,
            rotation_degrees: 30.0,
            style: V7ElementStyle::default(),
        },
        WritableElement2D::Arc {
            center: Point2 { x: -5.0, y: 6.0 },
            primary_axis: 3.0,
            secondary_axis: 1.5,
            rotation_degrees: 15.0,
            start_angle_degrees: 45.0,
            sweep_angle_degrees: -90.0,
            style: V7ElementStyle::default(),
        },
        WritableElement2D::Text {
            origin: Point2 { x: 2.0, y: 8.0 },
            text: b"Phase 5".to_vec(),
            font_id: 3,
            justification: 7,
            length_multiplier: 0.5,
            height_multiplier: 1.0,
            rotation_degrees: 10.0,
            style: V7ElementStyle::default(),
        },
    ];

    let output = write_v7_2d(SEED_2D, &elements, V7WriteOptions::default()).unwrap();
    assert_eq!(&output[output.len() - 2..], &[0xff, 0xff]);
    let document = read_v7_2d(&output, ScanOptions::default()).unwrap();
    assert_eq!(document.elements.len(), 10);
    assert_eq!(
        document.elements[3..]
            .iter()
            .map(|element| element.data.kind())
            .collect::<Vec<_>>(),
        [
            "LINE",
            "LINE_STRING",
            "SHAPE",
            "CURVE",
            "ELLIPSE",
            "ARC",
            "TEXT"
        ]
    );

    let line_header = document.elements[3].common_header.unwrap();
    assert_eq!(line_header.symbology.color, 12);
    assert_eq!(line_header.symbology.style, 3);
    assert_eq!(line_header.symbology.weight, 5);
    assert_eq!(line_header.graphic_group, 42);
    let ElementData2D::Line(ref line) = document.elements[3].data else {
        panic!("expected line")
    };
    assert_close(line.start_master.unwrap().x, 1.25);
    assert_close(line.start_master.unwrap().y, -2.5);
    assert_close(line.end_master.unwrap().x, 4.5);
    assert_close(line.end_master.unwrap().y, 3.75);

    let ElementData2D::Shape(ref shape) = document.elements[5].data else {
        panic!("expected shape")
    };
    assert_eq!(shape.vertices_uor.first(), shape.vertices_uor.last());
    let fill = &document.elements[5].linkages[0];
    assert_eq!(fill.data.kind(), "SHAPE_FILL");

    let ElementData2D::Ellipse(ref ellipse) = document.elements[7].data else {
        panic!("expected ellipse")
    };
    assert_close(ellipse.center_master.unwrap().x, 10.0);
    assert_close(ellipse.center_master.unwrap().y, 20.0);
    assert_close(ellipse.primary_axis_master.unwrap(), 4.0);
    assert_close(ellipse.secondary_axis_master.unwrap(), 2.0);
    assert_close(ellipse.rotation_degrees, 30.0);

    let ElementData2D::Arc(ref arc) = document.elements[8].data else {
        panic!("expected arc")
    };
    assert_close(arc.start_angle_degrees, 45.0);
    assert_close(arc.sweep_angle_degrees, -90.0);

    let ElementData2D::Text(ref text) = document.elements[9].data else {
        panic!("expected text")
    };
    assert_eq!(text.text_bytes, b"Phase 5");
    assert_eq!(text.font_id, 3);
    assert_eq!(text.justification, 7);
    assert_close(text.length_multiplier_master.unwrap(), 0.5);
    assert_close(text.height_multiplier_master.unwrap(), 1.0);
}

#[test]
fn phase_five_seed_copy_options_are_explicit_and_bounded() {
    let line = [WritableElement2D::Line {
        start: Point2 { x: 0.0, y: 0.0 },
        end: Point2 { x: 1.0, y: 1.0 },
        style: V7ElementStyle::default(),
    }];

    let minimal = write_v7_2d(SEED_2D, &line, V7WriteOptions::default()).unwrap();
    assert_eq!(
        scan_records(&minimal, ScanOptions::default())
            .unwrap()
            .records
            .len(),
        4
    );

    let whole = write_v7_2d(
        SEED_2D,
        &line,
        V7WriteOptions {
            copy_color_table: true,
            copy_seed_elements: true,
        },
    )
    .unwrap();
    assert_eq!(
        scan_records(&whole, ScanOptions::default())
            .unwrap()
            .records
            .len(),
        13
    );

    let mut color_body = vec![0_u8; 770];
    color_body[8..11].copy_from_slice(&[10, 20, 30]);
    let mut color_seed = SEED_2D[..2048].to_vec();
    color_seed.extend_from_slice(&synthetic_record(5, 1, &color_body, false, &[]));
    color_seed.extend_from_slice(&[0xff, 0xff]);
    let colored = write_v7_2d(&color_seed, &line, V7WriteOptions::default()).unwrap();
    let document = read_v7_2d(&colored, ScanOptions::default()).unwrap();
    assert_eq!(document.elements.len(), 5);
    assert!(document.active_color_table.is_some());
}

#[test]
fn phase_five_rejects_invalid_seeds_and_unrepresentable_entities() {
    let line = [WritableElement2D::Line {
        start: Point2 { x: 0.0, y: 0.0 },
        end: Point2 { x: 1.0, y: 1.0 },
        style: V7ElementStyle::default(),
    }];
    assert!(matches!(
        write_v7_2d(SEED_3D, &line, V7WriteOptions::default()),
        Err(DgnError::UnsupportedDimension {
            dimension: V7Dimension::Three
        })
    ));
    assert!(matches!(
        write_v7_2d(KNOT_OOB, &line, V7WriteOptions::default()),
        Err(DgnError::InvalidWriterSeed { .. })
    ));

    let too_many = [WritableElement2D::LineString {
        vertices: vec![Point2 { x: 0.0, y: 0.0 }; 102],
        style: V7ElementStyle::default(),
    }];
    assert!(matches!(
        write_v7_2d(SEED_2D, &too_many, V7WriteOptions::default()),
        Err(DgnError::InvalidWriterEntity {
            entity: "LINE_STRING",
            ..
        })
    ));

    let non_finite = [WritableElement2D::Line {
        start: Point2 {
            x: f64::NAN,
            y: 0.0,
        },
        end: Point2 { x: 1.0, y: 1.0 },
        style: V7ElementStyle::default(),
    }];
    assert!(matches!(
        write_v7_2d(SEED_2D, &non_finite, V7WriteOptions::default()),
        Err(DgnError::WriterCoordinateOutOfRange {
            entity: "LINE",
            axis: "x"
        })
    ));

    let tiny_sweep = [WritableElement2D::Arc {
        center: Point2 { x: 0.0, y: 0.0 },
        primary_axis: 1.0,
        secondary_axis: 1.0,
        rotation_degrees: 0.0,
        start_angle_degrees: 0.0,
        sweep_angle_degrees: 1.0e-8,
        style: V7ElementStyle::default(),
    }];
    assert!(matches!(
        write_v7_2d(SEED_2D, &tiny_sweep, V7WriteOptions::default()),
        Err(DgnError::InvalidWriterEntity { entity: "ARC", .. })
    ));
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}

fn with_synthetic_records(records: &[Vec<u8>]) -> Vec<u8> {
    let mut data = SMALLTEST[..10_136].to_vec();
    for record in records {
        data.extend_from_slice(record);
    }
    data.extend_from_slice(&[0xff, 0xff]);
    data
}

fn synthetic_record(
    element_type: u8,
    level: u8,
    body: &[u8],
    complex: bool,
    linkages: &[u8],
) -> Vec<u8> {
    let semantic_size = 36 + body.len();
    let size = semantic_size + linkages.len();
    assert_eq!(size % 2, 0);
    let mut record = vec![0; size];
    record[0] = level | if complex { 0x80 } else { 0 };
    record[1] = element_type;
    record[2..4].copy_from_slice(&u16::try_from(size / 2 - 2).unwrap().to_le_bytes());
    record[36..semantic_size].copy_from_slice(body);
    if !linkages.is_empty() {
        record[30..32].copy_from_slice(
            &u16::try_from((semantic_size - 32) / 2)
                .unwrap()
                .to_le_bytes(),
        );
        record[32..34].copy_from_slice(&0x0800_u16.to_le_bytes());
        record[semantic_size..].copy_from_slice(linkages);
    }
    record
}

fn put_middle_i32(bytes: &mut [u8], offset: usize, value: i32) {
    let value = value as u32;
    bytes[offset..offset + 4].copy_from_slice(&[
        (value >> 16) as u8,
        (value >> 24) as u8,
        value as u8,
        (value >> 8) as u8,
    ]);
}
