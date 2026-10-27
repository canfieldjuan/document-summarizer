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
