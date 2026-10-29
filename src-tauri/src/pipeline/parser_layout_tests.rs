// Public, offline probes for the accepted reading-order contract.
fn layout_text(font: &str, size: i64, x: i64, y: i64, text: &str) -> Vec<Operation> {
    vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec![font.into(), size.into()]),
        Operation::new("Td", vec![x.into(), y.into()]),
        Operation::new("Tj", vec![Object::string_literal(text)]),
        Operation::new("ET", vec![]),
    ]
}

fn layout_pdf(operations: Vec<Operation>) -> PdfDocument {
    let mut document = build_pdf(vec![FixturePage::Empty]);
    let page = document.get_pages()[&1];
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        Content { operations }.encode().unwrap(),
    ));
    document
        .get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Contents", content_id);
    document
}

#[test]
#[ignore = "accepted layout fail-first; run explicitly before parser adoption"]
fn native_reading_order_matches_visual_rows() {
    let mut operations = layout_text("F1", 12, 72, 650, "1. Payment");
    operations.extend(layout_text("F1", 12, 72, 630, "Payment is due."));
    operations.extend(layout_text("F1", 12, 72, 720, "Address: 42 Maple St."));
    let mut document = layout_pdf(operations);
    if let Some(root) = std::env::var_os("DOC_SUM_LAYOUT_PROBE_OUTPUT") {
        document
            .save(Path::new(&root).join("drawing-order.pdf"))
            .unwrap();
    }
    let pdf = save_pdf(document);
    let (ingested, _) = prepare_pdf_ingestion_with_source_type(
        pdf.0.to_str().unwrap(),
        None,
        SourceType::NativeText,
    )
    .unwrap();
    let parsed = PdfExtractParser::new().parse(&ingested).unwrap();
    assert_eq!(
        parsed.pages[0].text,
        "Address: 42 Maple St.\n1. Payment\nPayment is due."
    );
}

#[derive(Default)]
struct LayoutEvents(Vec<serde_json::Value>, bool);

impl pdf_extract::OutputDev for LayoutEvents {
    fn wants_geometry(&self) -> bool {
        self.1
    }
    fn begin_page(
        &mut self,
        page: u32,
        media: &pdf_extract::MediaBox,
        art: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), pdf_extract::OutputError> {
        self.0.push(serde_json::json!([
            "page", page, media.llx, media.lly, media.urx, media.ury, art
        ]));
        Ok(())
    }
    fn end_page(&mut self) -> Result<(), pdf_extract::OutputError> {
        self.0.push(serde_json::json!(["end_page"]));
        Ok(())
    }
    fn output_character(
        &mut self,
        trm: &pdf_extract::Transform,
        width: f64,
        spacing: f64,
        size: f64,
        text: &str,
    ) -> Result<(), pdf_extract::OutputError> {
        self.0.push(serde_json::json!([
            "char",
            [trm.m11, trm.m12, trm.m21, trm.m22, trm.m31, trm.m32],
            width,
            spacing,
            size,
            text
        ]));
        Ok(())
    }
    fn begin_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        self.0.push(serde_json::json!(["begin_word"]));
        Ok(())
    }
    fn end_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        self.0.push(serde_json::json!(["end_word"]));
        Ok(())
    }
    fn end_line(&mut self) -> Result<(), pdf_extract::OutputError> {
        self.0.push(serde_json::json!(["end_line"]));
        Ok(())
    }
}

fn font_metric_pdf(second_descent: i64) -> PdfDocument {
    let mut operations = layout_text("F1", 12, 72, 720, "A");
    operations.extend(layout_text("F2", 12, 96, 720, "B"));
    let mut document = layout_pdf(operations);
    let mut fonts = pdf_extract::Dictionary::new();
    for (name, descent) in [("F1", -200), ("F2", second_descent)] {
        let descriptor = document.add_object(dictionary! {
            "Type" => "FontDescriptor", "FontName" => "LayoutProbe",
            "Flags" => 32, "FontBBox" => vec![0.into(), descent.into(), 1000.into(), 1000.into()],
            "ItalicAngle" => 0, "Ascent" => 800, "Descent" => descent,
            "CapHeight" => 700, "StemV" => 80,
        });
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "LayoutProbe",
            "Encoding" => "WinAnsiEncoding", "FirstChar" => 65, "LastChar" => 66,
            "Widths" => vec![500.into(),500.into()], "FontDescriptor" => descriptor,
        });
        fonts.set(name, font);
    }
    let page = document.get_pages()[&1];
    document
        .get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Resources", dictionary! { "Font" => fonts });
    document
}

#[test]
#[ignore = "offline geometry-sufficiency probe; explicit durable output required"]
fn reading_order_geometry_sufficiency_probe() {
    let root = PathBuf::from(std::env::var_os("DOC_SUM_LAYOUT_PROBE_OUTPUT").unwrap());
    let mut traces = Vec::new();
    for (name, descent) in [("same-descent", -200), ("different-descent", -800)] {
        let mut document = font_metric_pdf(descent);
        let path = root.join(format!("{name}.pdf"));
        document.save(&path).unwrap();
        let loaded = PdfDocument::load(&path).unwrap();
        let mut events = LayoutEvents::default();
        output_doc_page(&loaded, &mut events, 1).unwrap();
        fs::write(
            root.join(format!("{name}-events.json")),
            serde_json::to_vec_pretty(&events.0).unwrap(),
        )
        .unwrap();
        traces.push(events.0);
    }
    assert_eq!(traces[0], traces[1], "descent affects callback information");
    println!("GEOMETRY_PROBE: both PDFs produce identical complete OutputDev event streams");
}

#[test]
#[ignore = "freeze legacy decoder callbacks before the accepted opt-in dependency patch"]
fn capture_layout_legacy_reference() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/reference");
    let output = PathBuf::from(std::env::var_os("DOC_SUM_LAYOUT_PROBE_OUTPUT").unwrap());
    let snapshots = collect_legacy_layout_snapshots(&source);
    fs::write(
        output.join("legacy.json"),
        serde_json::to_vec_pretty(&snapshots).unwrap(),
    )
    .unwrap();
    println!("LEGACY_FREEZE: {} public PDFs", snapshots.len());
}

fn collect_legacy_layout_snapshots(source: &Path) -> Vec<serde_json::Value> {
    let mut paths: Vec<_> = fs::read_dir(source)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "pdf"))
        .collect();
    paths.sort();
    let mut snapshots = Vec::new();
    for path in paths {
        let document = PdfDocument::load(&path).unwrap();
        let mut events = LayoutEvents::default();
        let event_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            output_doc_page(&document, &mut events, 1)
        }));
        let mut text = String::new();
        let text_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            output_doc_page(
                &document,
                &mut pdf_extract::PlainTextOutput::new(&mut text),
                1,
            )
        }));
        let outcome =
            |result: std::thread::Result<Result<(), pdf_extract::OutputError>>| match result {
                Ok(Ok(())) => "ok".to_owned(),
                Ok(Err(error)) => format!("error: {error}"),
                Err(error) => format!(
                    "panic: {}",
                    error
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| error.downcast_ref::<&str>().copied())
                        .unwrap_or("non-string")
                ),
            };
        let event_outcome = outcome(event_result);
        let text_outcome = outcome(text_result);
        if event_outcome != "ok" || text_outcome != "ok" {
            println!(
                "LEGACY_FAILURE: {} events={event_outcome} text={text_outcome}",
                path.file_stem().unwrap().to_str().unwrap()
            );
        }
        snapshots.push(serde_json::json!({
            "alias": path.file_stem().unwrap().to_str().unwrap(),
            "events": events.0,
            "text": text,
            "event_outcome": event_outcome,
            "text_outcome": text_outcome,
        }));
    }
    snapshots
}

#[test]
fn native_layout_legacy_compatibility() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/reference");
    // Preserve exact serialized numbers. Re-parsing without serde_json's
    // float_roundtrip feature can itself round an f64 and invent a difference.
    let expected = fs::read(root.join("legacy.json")).unwrap();
    let actual = serde_json::to_vec_pretty(&collect_legacy_layout_snapshots(&root)).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn decoder_geometry_matches_frozen_public_reference() {
    compare_frozen_geometry(None);
}

#[test]
fn decoder_geometry_base14_matches_frozen_reference() {
    compare_frozen_geometry(Some("base14"));
}

#[test]
fn decoder_geometry_retains_descent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/reference");
    for (alias, descent, lower) in [
        ("same-descent", -200.0, 717.6),
        ("different-descent", -800.0, 710.4),
    ] {
        let document =
            pdf_extract::geometry::PreciseDocument::load(root.join(format!("{alias}.pdf")))
                .unwrap();
        let page = pdf_extract::geometry::extract_page_geometry(&document, 1).unwrap();
        assert_eq!(page.glyphs[1].text, "B");
        assert_eq!(page.glyphs[1].descent, descent);
        assert!((page.glyphs[1].bounds[1] - lower).abs() <= 1e-9, "{alias}");
    }
}

#[test]
fn decoder_precision_preserves_line_boundary() {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/precision-collision");
    let mut traces = Vec::new();
    let mut pages = Vec::new();
    for alias in ["at-boundary", "over-boundary"] {
        let document =
            pdf_extract::geometry::PreciseDocument::load(root.join(format!("{alias}.pdf")))
                .unwrap();
        let geometry = pdf_extract::geometry::extract_page_geometry(&document, 1).unwrap();
        println!("{alias}: glyph B matrix y={}", geometry.glyphs[1].matrix[5]);
        let expected_y = if alias == "at-boundary" {
            717.0
        } else {
            716.99999
        };
        assert_eq!(geometry.glyphs[1].matrix[5], expected_y);
        let chars: Vec<_> = geometry
            .glyphs
            .iter()
            .map(|g| {
                serde_json::json!({
                    "text": g.text, "x0": g.bounds[0], "x1": g.bounds[2],
                    "top": 792.0-g.bounds[3], "bottom": 792.0-g.bounds[1],
                    "doctop": 792.0-g.bounds[3], "upright": g.upright, "size": g.size,
                })
            })
            .collect();
        pages.push(serde_json::json!({"alias": alias, "chars": chars}));
        traces.push(format!("{:?}", geometry.glyphs));
    }
    if let Some(output) = std::env::var_os("DOC_SUM_LAYOUT_PROBE_OUTPUT") {
        fs::write(
            Path::new(&output).join("precision-boundary-geometry.json"),
            serde_json::to_vec_pretty(&pages).unwrap(),
        )
        .unwrap();
    }
    assert_ne!(
        traces[0], traces[1],
        "distinct line-boundary inputs collapsed before the enriched glyph interface"
    );
}

#[test]
#[ignore = "explicit Phase A transformed-content/page-continuation qualification"]
fn decoder_geometry_continuation_reference() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/continuation");
    compare_geometry_root(&root, None);
}

fn compare_frozen_geometry(prefix: Option<&str>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/reference");
    compare_geometry_root(&root, prefix);
}

fn compare_geometry_root(root: &Path, prefix: Option<&str>) {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
    let tolerance = manifest["geometry_comparison"]["absolute_tolerance"]
        .as_f64()
        .unwrap();
    let mut differences = Vec::new();
    let mut actual_pages = Vec::new();
    for fixture in manifest["fixtures"].as_array().unwrap() {
        let alias = fixture["alias"].as_str().unwrap();
        if prefix.is_some_and(|prefix| !alias.starts_with(prefix)) {
            continue;
        }
        let reference: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(format!("{alias}.json"))).unwrap()).unwrap();
        let document =
            pdf_extract::geometry::PreciseDocument::load(root.join(format!("{alias}.pdf")))
                .unwrap();
        let mut initial_doctop = 0.0;
        for page in reference["pages"].as_array().unwrap() {
            let page_number = page["page_number"].as_u64().unwrap() as u32;
            let geometry =
                pdf_extract::geometry::extract_page_geometry(&document, page_number).unwrap();
            let expected = page["chars"].as_array().unwrap();
            assert_eq!(
                geometry.glyphs.len(),
                expected.len(),
                "{alias}: glyph count"
            );
            let [x0, y0, x1, y1] = geometry.media_box;
            let (mb_x0, mb_top, height) = if geometry.rotation == 90 || geometry.rotation == 270 {
                (y0.min(y1), -x0.max(x1) + (x1 - x0).abs(), (x1 - x0).abs())
            } else {
                (x0.min(x1), -y0.max(y1) + (y1 - y0).abs(), (y1 - y0).abs())
            };
            let mut chars = Vec::new();
            for (index, (glyph, wanted)) in geometry.glyphs.iter().zip(expected).enumerate() {
                let [left, bottom, right, top] = glyph.bounds;
                let actual = serde_json::json!({
                    "text": glyph.text, "fontname": glyph.font_name, "matrix": glyph.matrix,
                    "x0": left+mb_x0, "x1": right+mb_x0, "y0": bottom, "y1": top,
                    "top": height-top+mb_top, "bottom": height-bottom+mb_top,
                    "doctop": height-top+mb_top+initial_doctop, "width": right-left, "height": top-bottom,
                    "size": glyph.size, "adv": glyph.advance, "upright": glyph.upright,
                });
                for (key, value) in wanted.as_object().unwrap() {
                    if let Some(wanted) = value.as_f64() {
                        let got = actual[key].as_f64();
                        if !got
                            .is_some_and(|got| got.is_finite() && (got - wanted).abs() <= tolerance)
                        {
                            differences.push(format!(
                                "{alias}[{index}].{key}: expected={wanted} actual={got:?}"
                            ));
                        }
                    } else if key == "matrix" {
                        for (i, value) in value.as_array().unwrap().iter().enumerate() {
                            let wanted = value.as_f64().unwrap();
                            let got = actual[key][i].as_f64().unwrap();
                            if !got.is_finite() || (got - wanted).abs() > tolerance {
                                differences.push(format!(
                                    "{alias}[{index}].matrix[{i}]: expected={wanted} actual={got}"
                                ));
                            }
                        }
                    } else if actual[key] != *value {
                        differences.push(format!(
                            "{alias}[{index}].{key}: expected={value} actual={}",
                            actual[key]
                        ));
                    }
                }
                assert_eq!(glyph.occurrence, index);
                chars.push(actual);
            }
            actual_pages.push(
                serde_json::json!({"alias": alias, "page_number": page_number, "chars": chars}),
            );
            initial_doctop += height;
        }
    }
    if let Some(output) = std::env::var_os("DOC_SUM_LAYOUT_PROBE_OUTPUT") {
        let output = Path::new(&output);
        fs::write(
            output.join(format!("{}candidate-geometry.json", prefix.unwrap_or(""))),
            serde_json::to_vec_pretty(&actual_pages).unwrap(),
        )
        .unwrap();
        fs::write(
            output.join(format!("{}geometry-differences.json", prefix.unwrap_or(""))),
            serde_json::to_vec_pretty(&differences).unwrap(),
        )
        .unwrap();
    }
    assert!(
        differences.is_empty(),
        "{} geometry differences:\n{}",
        differences.len(),
        differences.join("\n")
    );
}

// Public API coverage for the vendored numeric owner. Handwritten object bytes
// ensure the writer cannot pre-round the values being tested.
fn precision_fixture(compressed: bool) -> Vec<u8> {
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0.00001 -0.00001 612.00001 792.00001] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Probe 7 0 R /Contents 4 0 R >>".to_vec(),
        b"<< /Length 24 >>\nstream\nBT 72 716.99999 Td (B) Tj\nendstream".to_vec(),
        b"<< /Type /Font /Subtype /Type3 /FontMatrix [0.00100001 0 0 0.00200001 0 0] /Widths [716.99999] >>".to_vec(),
        b"<< /Type /ObjStm /N 1 /First 4 /Length 61 >>\nstream\n7 0 << /Nested [716.99999 << /Negative -716.99999 /Zero -0.0 >>] >>\nendstream".to_vec(),
        b"<< /Nested [716.99999 << /Negative -716.99999 /Zero -0.0 >>] >>".to_vec(),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0usize];
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        if compressed && index == 6 {
            continue;
        }
        // Calculate each stream's actual byte length independently of the literals.
        let body = if let Some(start) = body.windows(7).position(|w| w == b"stream\n") {
            let start = start + 7;
            let end = body.windows(10).position(|w| w == b"\nendstream").unwrap();
            let header = std::str::from_utf8(&body[..start - 7]).unwrap();
            let length_start = header.find("/Length ").unwrap() + 8;
            let length_end = length_start + header[length_start..].find(' ').unwrap();
            let mut fixed = header.to_owned();
            fixed.replace_range(length_start..length_end, &(end - start).to_string());
            [fixed.as_bytes(), &b"stream\n"[..], &body[start..]].concat()
        } else {
            body.clone()
        };
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    offsets.push(xref);
    let mut entries = Vec::new();
    for (id, offset) in offsets.iter().enumerate() {
        let (kind, first, second): (u8, u32, u16) = if id == 0 {
            (0, 0, 65535)
        } else if compressed && id == 7 {
            (2, 6, 0)
        } else {
            (1, *offset as u32, 0)
        };
        entries.push(kind);
        entries.extend_from_slice(&first.to_be_bytes());
        entries.extend_from_slice(&second.to_be_bytes());
    }
    bytes.extend_from_slice(format!("8 0 obj\n<< /Type /XRef /Size 9 /Root 1 0 R /Probe 716.99999 /W [1 4 2] /Length {} >>\nstream\n", entries.len()).as_bytes());
    bytes.extend_from_slice(&entries);
    bytes.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    bytes
}

#[test]
fn decoder_precision_propagates_nested_and_compressed_objects_without_global_state() {
    use lopdf::{LoadOptions, NumericPrecision, Object};
    for compressed in [false, true] {
        let input = precision_fixture(compressed);
        for precision in [
            NumericPrecision::Precise,
            NumericPrecision::Legacy,
            NumericPrecision::Precise,
        ] {
            let doc = PdfDocument::load_mem_with_options(
                &input,
                LoadOptions {
                    numeric_precision: precision,
                    ..Default::default()
                },
            )
            .unwrap();
            let expected = if precision == NumericPrecision::Precise {
                716.99999
            } else {
                717.0
            };
            let nested = doc
                .get_dictionary((7, 0))
                .unwrap()
                .get(b"Nested")
                .unwrap()
                .as_array()
                .unwrap();
            assert_eq!(nested[0].as_float64().unwrap(), expected);
            assert_eq!(
                nested[1]
                    .as_dict()
                    .unwrap()
                    .get(b"Negative")
                    .unwrap()
                    .as_float64()
                    .unwrap(),
                -expected
            );
            assert!(nested[1]
                .as_dict()
                .unwrap()
                .get(b"Zero")
                .unwrap()
                .as_float64()
                .unwrap()
                .is_sign_negative());
            assert_eq!(
                doc.trailer.get(b"Probe").unwrap().as_float64().unwrap(),
                expected
            );
            let operands = doc.get_and_decode_page_content((3, 0)).unwrap().operations;
            assert_eq!(operands[1].operands[1].as_float64().unwrap(), expected);
            let matrix = doc
                .get_dictionary((5, 0))
                .unwrap()
                .get(b"FontMatrix")
                .unwrap()
                .as_array()
                .unwrap();
            let media = doc
                .get_dictionary((2, 0))
                .unwrap()
                .get(b"MediaBox")
                .unwrap()
                .as_array()
                .unwrap();
            if precision == NumericPrecision::Precise {
                assert!(matches!(nested[0], Object::Real64(_)));
                assert!(nested[0].as_f32().is_err());
                assert!(nested[0].as_float().is_err());
                assert_eq!(matrix[0].as_float64().unwrap(), 0.00100001);
                assert_eq!(media[2].as_float64().unwrap(), 612.00001);
            } else {
                assert!(matches!(nested[0], Object::Real(_)));
                assert_eq!(matrix[0].as_float().unwrap(), 0.00100001_f32);
                assert_eq!(media[2].as_float().unwrap(), 612.00001_f32);
            }
        }
    }
    assert!(matches!(
        lopdf::Object::from(716.99999_f64),
        lopdf::Object::Real(717.0)
    ));
}

#[test]
fn decoder_precision_content_grammar_and_serialization() {
    use lopdf::{content::Content, NumericPrecision, Object};
    let bytes = b"+716.99999 -716.99999 -0.0 .12 10. [716.99999 << /A 0.00100001 >>] cm";
    for precision in [NumericPrecision::Legacy, NumericPrecision::Precise] {
        let content = Content::decode_strict_with_precision(bytes, precision).unwrap();
        let operands = &content.operations[0].operands;
        let expected = if precision == NumericPrecision::Precise {
            716.99999
        } else {
            717.0
        };
        assert_eq!(operands[0].as_float64().unwrap(), expected);
        assert_eq!(operands[1].as_float64().unwrap(), -expected);
        assert!(operands[2].as_float64().unwrap().is_sign_negative());
        assert_eq!(
            operands[5].as_array().unwrap()[0].as_float64().unwrap(),
            expected
        );
        let encoded = content.encode().unwrap();
        let again = Content::decode_strict_with_precision(&encoded, precision).unwrap();
        assert_eq!(
            again.operations[0].operands[0].as_float64().unwrap(),
            expected
        );
        if precision == NumericPrecision::Precise {
            assert!(String::from_utf8(encoded)
                .unwrap()
                .starts_with("716.99999 -716.99999"));
            assert_eq!(format!("{:?}", Object::Real64(716.99999)), "716.99999");
        } else {
            assert_eq!(
                encoded,
                Content::decode_strict(bytes).unwrap().encode().unwrap()
            );
        }
        for malformed in [
            b"--.12 cm".as_slice(),
            b". cm",
            b"[1.0 cm",
            b"1.0 cm @",
            b"1.0 cm 2.0",
        ] {
            assert!(
                Content::decode_strict_with_precision(malformed, precision).is_err(),
                "{malformed:?}"
            );
        }
        // The existing grammar admits adjacent decimal operands and reads e as
        // an operator, not exponent notation. Preserve that tokenization.
        let adjacent = Content::decode_strict_with_precision(b"1.2.3 cm", precision).unwrap();
        assert_eq!(adjacent.operations[0].operands.len(), 2);
        let exponent = Content::decode_strict_with_precision(b"1e-5 cm", precision).unwrap();
        assert_eq!(exponent.operations[0].operator, "e");
        assert!(matches!(
            exponent.operations[0].operands[0],
            Object::Integer(1)
        ));
        assert!(Content::decode_strict_with_precision(b"", precision)
            .unwrap()
            .operations
            .is_empty());
    }
}

#[test]
fn decoder_precision_serialization_preserves_negative_zero() {
    use lopdf::{
        content::{Content, Operation},
        NumericPrecision, Object,
    };
    let content = Content {
        operations: vec![Operation::new(
            "cm",
            vec![Object::Real64(-0.0), Object::Real64(717.0)],
        )],
    };
    let encoded = content.encode().unwrap();
    let decoded =
        Content::decode_strict_with_precision(&encoded, NumericPrecision::Precise).unwrap();
    assert!(matches!(decoded.operations[0].operands[0], Object::Real64(v) if v.is_sign_negative()));
    assert!(matches!(
        decoded.operations[0].operands[1],
        Object::Real64(717.0)
    ));
    for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let invalid = Content {
            operations: vec![Operation::new("cm", vec![Object::Real64(value)])],
        };
        assert!(invalid.encode().is_err());
    }
}

#[test]
fn decoder_precision_survives_deferred_decryption_of_object_streams() {
    use lopdf::{
        EncryptionState, EncryptionVersion, LoadOptions, NumericPrecision, Object, Permissions,
    };
    for precision in [NumericPrecision::Precise, NumericPrecision::Legacy] {
        let mut doc = PdfDocument::load_mem_with_options(
            &precision_fixture(true),
            LoadOptions {
                numeric_precision: precision,
                ..Default::default()
            },
        )
        .unwrap();
        doc.trailer.set(
            "ID",
            Object::Array(vec![
                Object::string_literal("public-test-id"),
                Object::string_literal("public-test-id"),
            ]),
        );
        // Only the compressed object remains, so decrypt must use that reader.
        doc.objects.remove(&(7, 0));
        let state = EncryptionState::try_from(EncryptionVersion::V1 {
            document: &doc,
            owner_password: "public-owner",
            user_password: "public-user",
            permissions: Permissions::all(),
        })
        .unwrap();
        doc.encrypt(&state).unwrap();
        assert!(doc.decrypt("wrong-public-password").is_err());
        doc.decrypt("public-user").unwrap();
        let number = &doc
            .get_dictionary((7, 0))
            .unwrap()
            .get(b"Nested")
            .unwrap()
            .as_array()
            .unwrap()[0];
        let expected = if precision == NumericPrecision::Precise {
            716.99999
        } else {
            717.0
        };
        assert_eq!(number.as_float64().unwrap(), expected);
    }
}

#[test]
fn decoder_precision_legacy_callback_cannot_request_precise_geometry() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("vendor/pdf-extract/tests/precision-collision/over-boundary.pdf");
    let legacy = PdfDocument::load(path).unwrap();
    let error = output_doc_page(&legacy, &mut LayoutEvents(Vec::new(), true), 1).unwrap_err();
    assert!(error.to_string().contains("precise document"));
}

#[test]
fn decoder_form_direct_empty_resource_reference() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/forms");
    compare_geometry_root(&root, Some("direct-empty-resources"));
}

#[test]
fn decoder_form_invocation_reference() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/forms");
    compare_geometry_root(&root, None);
}

#[test]
fn decoder_form_malformed_invocations_return_errors() {
    check_form_errors(false);
}

#[test]
fn decoder_form_cycles_return_errors() {
    check_form_errors(true);
}

fn check_form_errors(cycles: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/forms");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
    let mut failures = Vec::new();
    for case in manifest["errors"].as_array().unwrap() {
        let alias = case["alias"].as_str().unwrap();
        if alias.contains("cycle") != cycles {
            continue;
        }
        let document =
            pdf_extract::geometry::PreciseDocument::load(root.join(format!("{alias}.pdf")))
                .unwrap();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pdf_extract::geometry::extract_page_geometry(&document, 1)
        })) {
            Ok(Err(_)) => {}
            Ok(Ok(_)) => {
                failures.push(format!("{alias}: returned successful partial/invalid page"))
            }
            Err(_) => failures.push(format!("{alias}: panicked instead of returning an error")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn decoder_form_deep_acyclic_invocations_and_repeat_are_valid() {
    let mut document = layout_pdf(vec![]);
    let font_id = document.add_object(
        dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"},
    );
    let mut target = document.add_object(Stream::new(
        dictionary! {
            "Subtype" => "Form", "BBox" => vec![0.into(),0.into(),612.into(),792.into()],
            "Resources" => dictionary! {"Font" => dictionary! {"F1" => font_id}}
        },
        Content {
            operations: layout_text("F1", 12, 72, 716, "A"),
        }
        .encode()
        .unwrap(),
    ));
    for _ in 0..4096 {
        target = document.add_object(Stream::new(
            dictionary! {
                "Subtype" => "Form", "BBox" => vec![0.into(),0.into(),612.into(),792.into()],
                "Resources" => dictionary! {"XObject" => dictionary! {"Child" => target}}
            },
            b"/Child Do".to_vec(),
        ));
    }
    let contents =
        document.add_object(Stream::new(dictionary! {}, b"/Child Do /Child Do".to_vec()));
    let page = document.get_pages()[&1];
    let dict = document
        .get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap();
    dict.set(
        "Resources",
        dictionary! {"XObject" => dictionary! {"Child" => target}},
    );
    dict.set("Contents", contents);
    let pdf = save_pdf(document);
    let precise = pdf_extract::geometry::PreciseDocument::load(&pdf.0).unwrap();
    for _ in 0..2 {
        let page = pdf_extract::geometry::extract_page_geometry(&precise, 1).unwrap();
        assert_eq!(
            page.glyphs
                .iter()
                .map(|g| g.text.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "A"]
        );
    }
}

#[test]
fn decoder_form_falsy_values_do_not_select_defaults() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/forms");
    let values = vec![
        Object::Boolean(false),
        Object::Integer(0),
        Object::string_literal(""),
        Object::Array(vec![]),
    ];
    for key in ["Resources", "Matrix"] {
        for value in &values {
            let mut document = PdfDocument::load(root.join("missing-matrix.pdf")).unwrap();
            document
                .get_object_mut((6, 0))
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .dict
                .set(key, value.clone());
            let pdf = save_pdf(document);
            let precise = pdf_extract::geometry::PreciseDocument::load(&pdf.0).unwrap();
            assert!(
                pdf_extract::geometry::extract_page_geometry(&precise, 1).is_err(),
                "{key}: {value:?}"
            );
        }
    }
    // A valid zero matrix is data, not an absent value selecting identity.
    let mut document = PdfDocument::load(root.join("missing-matrix.pdf")).unwrap();
    document
        .get_object_mut((6, 0))
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("Matrix", vec![Object::Integer(0); 6]);
    let pdf = save_pdf(document);
    let precise = pdf_extract::geometry::PreciseDocument::load(&pdf.0).unwrap();
    let page = pdf_extract::geometry::extract_page_geometry(&precise, 1).unwrap();
    assert!(!page.glyphs.is_empty());
    for glyph in page.glyphs {
        assert_eq!(glyph.matrix, [0.; 6]);
    }
}

#[test]
fn decoder_shared_gs_dispatch_preserves_existing_validation() {
    for enriched in [false, true] {
        for valid in [true, false] {
            let mut document = layout_pdf(vec![Operation::new("gs", vec!["State".into()])]);
            let page = document.get_pages()[&1];
            document.get_object_mut(page).unwrap().as_dict_mut().unwrap().set("Resources", dictionary! {
                "ExtGState" => dictionary! {"State" => dictionary! {
                    "Type" => if valid {"ExtGState"} else {"InvalidState"}, "SMask" => "None"
                }}
            });
            let pdf = save_pdf(document);
            let bytes = fs::read(&pdf.0).unwrap();
            let doc = PdfDocument::load_mem_with_options(
                &bytes,
                pdf_extract::LoadOptions {
                    numeric_precision: if enriched {
                        pdf_extract::NumericPrecision::Precise
                    } else {
                        pdf_extract::NumericPrecision::Legacy
                    },
                    ..Default::default()
                },
            )
            .unwrap();
            // ExtGState validation predates this repair; silently skipping gs changes it.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                output_doc_page(&doc, &mut LayoutEvents(Vec::new(), enriched), 1)
            }));
            assert_eq!(
                result.is_ok(),
                valid,
                "gs dispatch: enriched={enriched} valid={valid}"
            );
            if let Ok(result) = result {
                result.unwrap();
            }
        }
    }
}
