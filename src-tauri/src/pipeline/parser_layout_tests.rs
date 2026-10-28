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
struct LayoutEvents(Vec<serde_json::Value>);

impl pdf_extract::OutputDev for LayoutEvents {
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
        let document = PdfDocument::load(root.join(format!("{alias}.pdf"))).unwrap();
        let page = pdf_extract::geometry::extract_page_geometry(&document, 1).unwrap();
        assert_eq!(page.glyphs[1].text, "B");
        assert_eq!(page.glyphs[1].descent, descent);
        assert!((page.glyphs[1].bounds[1] - lower).abs() <= 1e-9, "{alias}");
    }
}

#[test]
#[ignore = "accepted r4 STOP: lopdf decimal precision loses the reference line boundary"]
fn decoder_precision_preserves_line_boundary() {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/precision-collision");
    let mut traces = Vec::new();
    for alias in ["at-boundary", "over-boundary"] {
        let document = PdfDocument::load(root.join(format!("{alias}.pdf"))).unwrap();
        let geometry = pdf_extract::geometry::extract_page_geometry(&document, 1).unwrap();
        println!("{alias}: glyph B matrix y={}", geometry.glyphs[1].matrix[5]);
        traces.push(format!("{:?}", geometry.glyphs));
    }
    assert_ne!(
        traces[0], traces[1],
        "distinct line-boundary inputs collapsed before the enriched glyph interface"
    );
}

fn compare_frozen_geometry(prefix: Option<&str>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdf-extract/tests/reference");
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
        let document = PdfDocument::load(root.join(format!("{alias}.pdf"))).unwrap();
        let geometry = pdf_extract::geometry::extract_page_geometry(&document, 1).unwrap();
        let page = &reference["pages"][0];
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
                "doctop": height-top+mb_top, "width": right-left, "height": top-bottom,
                "size": glyph.size, "adv": glyph.advance, "upright": glyph.upright,
            });
            for (key, value) in wanted.as_object().unwrap() {
                if let Some(wanted) = value.as_f64() {
                    let got = actual[key].as_f64();
                    if !got.is_some_and(|got| got.is_finite() && (got - wanted).abs() <= tolerance)
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
        actual_pages.push(serde_json::json!({"alias": alias, "chars": chars}));
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
