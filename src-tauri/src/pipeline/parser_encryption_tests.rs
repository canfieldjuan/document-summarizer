use crate::pipeline::contracts::DocumentNormalizer;
use crate::pipeline::normalize::CanonicalNormalizer;
use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter, IdentityCryptFilter};
use lopdf::{EncryptionState, EncryptionVersion, Permissions};
use std::sync::Arc;

#[test]
fn direct_encryption_dictionary_is_classified_by_its_contents() {
    let mut referenced = build_pdf(vec![FixturePage::Text(b"ENCRYPTED".to_vec())]);
    referenced.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    encrypt_rc4(&mut referenced, "");

    let encryption_id = referenced
        .trailer
        .get(b"Encrypt")
        .and_then(Object::as_reference)
        .expect("encrypted fixture reference");
    let dictionary = referenced
        .get_encrypted()
        .expect("complete Standard encryption dictionary")
        .clone();
    let control = save_pdf(referenced.clone());
    let mut conn = db::init_db(":memory:").unwrap();
    let parser = PdfExtractParser::new();
    let run = ingest_fixture(&mut conn, &control.0);
    let parsed = parse_document(&mut conn, &parser, &run.run_id).unwrap();
    assert_eq!(parsed.pages[0].text.trim(), "ENCRYPTED");

    let mut direct = referenced.clone();
    direct.objects.remove(&encryption_id);
    direct.trailer.set("Encrypt", dictionary);
    let mut missing = direct.clone();
    missing
        .trailer
        .get_mut(b"Encrypt")
        .and_then(Object::as_dict_mut)
        .expect("direct dictionary")
        .remove(b"U");
    let mut wrong_type = direct.clone();
    wrong_type
        .trailer
        .get_mut(b"Encrypt")
        .and_then(Object::as_dict_mut)
        .expect("direct dictionary")
        .set("P", "not-an-integer");
    let mut referenced_missing = referenced.clone();
    referenced_missing
        .get_dictionary_mut(encryption_id)
        .expect("referenced dictionary")
        .remove(b"U");
    let mut referenced_wrong_type = referenced;
    referenced_wrong_type
        .get_dictionary_mut(encryption_id)
        .expect("referenced dictionary")
        .set("P", "not-an-integer");

    for (case, document, expected) in [
        ("complete direct", direct, "PDF_ENCRYPTION_UNSUPPORTED"),
        ("missing direct field", missing, "MALFORMED_PDF"),
        ("wrong direct field type", wrong_type, "MALFORMED_PDF"),
        (
            "missing referenced field",
            referenced_missing,
            "MALFORMED_PDF",
        ),
        (
            "wrong referenced field type",
            referenced_wrong_type,
            "MALFORMED_PDF",
        ),
    ] {
        assert_parse_refusal(&mut conn, &pdf_bytes(document), case, expected);
    }
}

#[test]
fn unsupported_standard_revision_fails_as_unsupported_encryption() {
    let mut pdf = build_pdf(vec![FixturePage::Text(b"unsupported revision".to_vec())]);
    encrypt_rc4(&mut pdf, "");
    let encrypt_id = pdf.trailer.get(b"Encrypt").unwrap().as_reference().unwrap();
    let mut conn = db::init_db(":memory:").unwrap();
    for (field, value) in [("R", 1), ("V", 6)] {
        let mut unsupported = pdf.clone();
        // Change one field on the known-good dictionary.
        unsupported
            .get_dictionary_mut(encrypt_id)
            .unwrap()
            .set(field, value);
        assert_parse_refusal(
            &mut conn,
            &pdf_bytes(unsupported),
            field,
            "PDF_ENCRYPTION_UNSUPPORTED",
        );
    }
}

#[test]
fn public_key_encryption_is_unsupported_in_both_trailer_forms() {
    let mut direct = build_pdf(vec![FixturePage::Text(b"public-key handler".to_vec())]);
    direct.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    set_file_ids(&mut direct);
    // Handler classification happens before recipient decryption. This fixture
    // supplies a typed recipient entry without requiring a private key.
    let dictionary = dictionary! {
        "Filter" => "Adobe.PubSec",
        "SubFilter" => "adbe.pkcs7.s4",
        "V" => 2,
        "Length" => 128,
        "Recipients" => vec![Object::string_literal(b"synthetic recipient".to_vec())],
    };
    let mut referenced = direct.clone();
    direct.trailer.set("Encrypt", dictionary.clone());
    let id = referenced.add_object(dictionary);
    referenced.trailer.set("Encrypt", id);

    let mut conn = db::init_db(":memory:").unwrap();
    for (case, pdf) in [("direct", direct), ("referenced", referenced)] {
        let bytes = pdf_bytes(pdf);
        assert!(
            matches!(
                PdfDocument::load_mem_checked_encryption(&bytes),
                Err(LopdfError::UnsupportedSecurityHandler(handler)) if handler == b"Adobe.PubSec"
            ),
            "{case}"
        );
        assert_parse_refusal(&mut conn, &bytes, case, "PDF_ENCRYPTION_UNSUPPORTED");
    }
}

#[test]
fn invalid_encrypt_reference_never_persists_a_parsed_artifact() {
    let mut document = build_pdf(vec![FixturePage::Text(b"PAGE".to_vec())]);
    let wrong_type = document.add_object(Object::Integer(17));
    let missing = (document.max_id + 16, 0);
    let invalid_entries = [
        (
            "direct dictionary",
            Object::Dictionary(dictionary! { "Filter" => "Standard" }),
        ),
        ("missing object", Object::Reference(missing)),
        ("wrong object type", Object::Reference(wrong_type)),
        ("non-dictionary entry", Object::Integer(17)),
    ];
    let mut conn = db::init_db(":memory:").expect("schema should initialize");

    for (case, entry) in invalid_entries {
        let mut malformed = document.clone();
        malformed.trailer.set("Encrypt", entry);
        assert_parse_refusal(&mut conn, &pdf_bytes(malformed), case, "MALFORMED_PDF");
    }
}

#[test]
fn encrypted_pdf_with_nonempty_password_reports_password_required() {
    let mut document = build_pdf(vec![FixturePage::Text(b"ENCRYPTED".to_vec())]);
    encrypt_rc4(&mut document, "user-password");
    let mut conn = db::init_db(":memory:").unwrap();
    let failure = assert_parse_refusal(
        &mut conn,
        &pdf_bytes(document),
        "nonempty user password",
        "PDF_PASSWORD_REQUIRED",
    );
    assert_eq!(
        failure.message,
        "This PDF requires a password. Password entry is not supported."
    );
}

#[test]
fn empty_owner_password_recovers_the_user_key_for_rc4() {
    let mut pdf = build_pdf(vec![FixturePage::Text(b"Owner opens this page".to_vec())]);
    set_file_ids(&mut pdf);
    let state = EncryptionState::try_from(EncryptionVersion::V1 {
        document: &pdf,
        owner_password: "",
        user_password: "nonempty-user",
        permissions: Permissions::PRINTABLE,
    })
    .unwrap();
    pdf.encrypt(&state).unwrap();
    assert!(pdf.authenticate_owner_password("").is_ok());
    assert!(matches!(
        pdf.authenticate_user_password(""),
        Err(LopdfError::Decryption(DecryptionError::IncorrectPassword))
    ));
    let bytes = pdf_bytes(pdf);
    let path = TestPath::write("pdf", &bytes);
    let mut conn = db::init_db(":memory:").unwrap();
    let run = ingest_fixture(&mut conn, &path.0);
    let parsed = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id).unwrap();
    assert_eq!(parsed.pages[0].text.trim(), "Owner opens this page");
}

#[test]
fn aes256_with_invalid_permissions_is_malformed_after_authentication() {
    use lopdf::encryption::crypt_filters::Aes256CryptFilter;
    let mut pdf = build_pdf(vec![FixturePage::Text(b"AES-256 synthetic page".to_vec())]);
    set_file_ids(&mut pdf);
    let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    let state = EncryptionState::try_from(EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
        file_encryption_key: &[42; 32],
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: "owner-password",
        user_password: "",
        permissions: Permissions::PRINTABLE,
    })
    .unwrap();
    pdf.encrypt(&state).unwrap();
    let valid = pdf_bytes(pdf.clone());
    let path = TestPath::write("pdf", &valid);
    let mut conn = db::init_db(":memory:").unwrap();
    let run = ingest_fixture(&mut conn, &path.0);
    let parsed = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id).unwrap();
    assert_eq!(parsed.pages[0].text.trim(), "AES-256 synthetic page");

    let id = pdf.trailer.get(b"Encrypt").unwrap().as_reference().unwrap();
    // Changing P leaves both password hashes intact but invalidates Perms.
    pdf.get_dictionary_mut(id)
        .unwrap()
        .set("P", (state.permissions().bits() ^ 4) as i64);
    assert!(pdf.authenticate_user_password("").is_ok());
    assert_parse_refusal(&mut conn, &pdf_bytes(pdf), "invalid Perms", "MALFORMED_PDF");
}

#[test]
fn retained_aes_failure_is_reported_before_partial_page_extraction() {
    let intact = include_bytes!("../../tests/fixtures/issue133-aes-intact.pdf");
    let corrupt = include_bytes!("../../tests/fixtures/issue133-aes-corrupt.pdf");
    let doc = PdfDocument::load_mem_checked_encryption(intact).unwrap();
    assert!(doc.was_encrypted());
    assert!(!doc.trailer.has(b"Encrypt"));
    let mut conn = db::init_db(":memory:").unwrap();
    let intact_path = TestPath::write("pdf", intact);
    let run = ingest_fixture(&mut conn, &intact_path.0);
    let parsed = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id).unwrap();
    assert_eq!(
        parsed
            .pages
            .iter()
            .map(|page| page.text.trim())
            .collect::<Vec<_>>(),
        ["INTACT_PAGE", "SECOND_PAGE"]
    );
    assert!(!parsed.requires_ocr());
    assert!(matches!(
        PdfDocument::load_mem_checked_encryption(corrupt),
        Err(LopdfError::Decryption(
            DecryptionError::InvalidCipherTextLength
        ))
    ));
    assert_parse_refusal(&mut conn, corrupt, "corrupt AES stream", "MALFORMED_PDF");
}

#[test]
fn encrypted_object_stream_materializes_referenced_pages() {
    let bytes = include_bytes!("../../tests/fixtures/issue133-aes-object-stream.pdf");
    let opened = PdfDocument::load_mem_checked_encryption(bytes).unwrap();
    assert!(opened.was_encrypted());
    assert!(opened
        .reference_table
        .entries
        .values()
        .any(|entry| { matches!(entry, lopdf::xref::XrefEntry::Compressed { .. }) }));
    let path = TestPath::write("pdf", bytes);
    let mut conn = db::init_db(":memory:").unwrap();
    let run = ingest_fixture(&mut conn, &path.0);
    let parsed = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id).unwrap();
    assert_eq!(parsed.pages.len(), 1);
    assert_eq!(
        parsed.pages[0].text.trim(),
        "Synthetic compressed encrypted page"
    );
}

#[test]
fn encrypted_object_stream_with_invalid_compression_is_malformed() {
    let bytes = include_bytes!("../../tests/fixtures/issue133-aes-corrupt-object-stream.pdf");
    let error = PdfDocument::load_mem_checked_encryption(bytes).unwrap_err();
    eprintln!("corrupt object stream failure: {error:?}");
    assert!(matches!(error, LopdfError::Decompress(_)));
    let mut conn = db::init_db(":memory:").unwrap();
    assert_parse_refusal(&mut conn, bytes, "corrupt object stream", "MALFORMED_PDF");
}

#[test]
fn encrypted_page_with_invalid_compression_is_malformed() {
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    let mut pdf = build_pdf(vec![
        FixturePage::Text(b"intact first page".to_vec()),
        FixturePage::Text(b"damaged second page".to_vec()),
    ]);
    let page_id = pdf.get_pages()[&2];
    let stream_id = pdf.get_page_contents(page_id)[0];
    let stream = pdf
        .get_object_mut(stream_id)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&stream.content).unwrap();
    let mut data = encoder.finish().unwrap();
    *data.last_mut().unwrap() ^= 1;
    stream.dict.set("Filter", "FlateDecode");
    stream.set_content(data);
    encrypt_rc4(&mut pdf, "");
    let mut conn = db::init_db(":memory:").unwrap();
    assert_parse_refusal(
        &mut conn,
        &pdf_bytes(pdf),
        "corrupt page Flate stream",
        "MALFORMED_PDF",
    );
}

#[test]
fn readable_encrypted_pdf_twins_preserve_text_normalization_and_source_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let clear_path = directory.path().join("clear.pdf");
    let rc4_path = directory.path().join("restricted.pdf");
    let aes_path = directory.path().join("source.pdf");
    let pdf = build_pdf(vec![
        FixturePage::Text(b"READABLE_TEXT".to_vec()),
        FixturePage::Text(b"caf\xe9 contract".to_vec()),
    ]);
    let clear = pdf_bytes(pdf.clone());
    let mut rc4 = pdf.clone();
    let mut aes = pdf;
    encrypt_rc4(&mut rc4, "");
    encrypt_aes(&mut aes);
    let mut conn = db::init_db(":memory:").unwrap();
    let parser = PdfExtractParser::new();
    let normalizer = CanonicalNormalizer::new();
    let mut results = Vec::new();
    for (path, bytes) in [
        (&clear_path, clear),
        (&rc4_path, pdf_bytes(rc4)),
        (&aes_path, pdf_bytes(aes)),
    ] {
        fs::write(path, &bytes).unwrap();
        let (source, run) = ingest_pdf(&mut conn, path.to_str().unwrap()).unwrap();
        let parsed = parse_document(&mut conn, &parser, &run.run_id).unwrap();
        assert!(!parsed.requires_ocr());
        assert_eq!(
            db::get_parsed_document(&conn, &run.run_id)
                .unwrap()
                .unwrap(),
            parsed
        );
        let normalized = normalizer.normalize(&parsed).unwrap();
        let spans = normalized
            .pages
            .iter()
            .map(|page| {
                let blocks = page
                    .content
                    .iter()
                    .map(|block| (block.kind.clone(), block.text.clone(), block.source.clone()))
                    .collect::<Vec<_>>();
                (
                    page.page_number,
                    blocks,
                    page.warnings.clone(),
                    page.requires_visual_processing,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(source.content_hash, format!("{:x}", Sha256::digest(&bytes)));
        let persisted = db::get_document(&conn, &source.document_id)
            .unwrap()
            .unwrap();
        assert_eq!(persisted.content_hash, source.content_hash);
        assert_eq!(persisted.byte_size, bytes.len() as u64);
        assert_eq!(persisted.local_source_path, source.local_source_path);
        results.push((source, parsed, normalized.warnings, spans));
    }
    let (clear_source, clear_parsed, clear_warnings, clear_spans) = &results[0];
    assert_eq!(clear_parsed.pages[1].text.trim(), "café contract");
    for (source, parsed, warnings, spans) in &results[1..] {
        assert_ne!(source.content_hash, clear_source.content_hash);
        assert_ne!(source.document_id, clear_source.document_id);
        assert_eq!(parsed.source_type, clear_parsed.source_type);
        assert_eq!(parsed.parser_id, clear_parsed.parser_id);
        assert_eq!(parsed.parser_version, clear_parsed.parser_version);
        assert_eq!(parsed.pages, clear_parsed.pages);
        assert_eq!(parsed.warnings, clear_parsed.warnings);
        assert_eq!(warnings, clear_warnings);
        assert_eq!(spans, clear_spans);
    }
    let files: HashSet<_> = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files, HashSet::from([clear_path, rc4_path, aes_path]));
}

#[test]
fn encrypted_scan_and_mixed_pages_keep_the_existing_visual_signals() {
    for unread in [Some(0), Some(1), Some(2), None] {
        let pages = (0..3)
            .map(|index| match unread {
                Some(unread) if unread != index => {
                    FixturePage::Text(format!("page {}", index + 1).into_bytes())
                }
                _ => FixturePage::ImageOnly,
            })
            .collect();
        let mut pdf = build_pdf(pages);
        let clear = save_pdf(pdf.clone());
        encrypt_rc4(&mut pdf, "");
        let encrypted = save_pdf(pdf);
        let mut conn = db::init_db(":memory:").unwrap();
        let parser = PdfExtractParser::new();
        let clear_run = ingest_fixture(&mut conn, &clear.0);
        let encrypted_run = ingest_fixture(&mut conn, &encrypted.0);
        let clear_parsed = parse_document(&mut conn, &parser, &clear_run.run_id).unwrap();
        let parsed = parse_document(&mut conn, &parser, &encrypted_run.run_id).unwrap();
        assert_eq!(parsed.pages, clear_parsed.pages);
        assert_eq!(parsed.warnings, clear_parsed.warnings);
        assert!(parsed.requires_ocr());
        assert_eq!(
            parsed
                .pages
                .iter()
                .map(|page| page.requires_visual_processing)
                .collect::<Vec<_>>(),
            (0..3)
                .map(|index| unread.is_none() || unread == Some(index))
                .collect::<Vec<_>>()
        );
    }
}

fn encrypt_aes(pdf: &mut PdfDocument) -> EncryptionState {
    set_file_ids(pdf);
    let filter: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);
    let state = EncryptionState::try_from(EncryptionVersion::V4 {
        document: pdf,
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: "owner-password",
        user_password: "",
        permissions: Permissions::PRINTABLE,
    })
    .unwrap();
    pdf.encrypt(&state).unwrap();
    state
}

fn assert_parse_refusal(
    conn: &mut Connection,
    bytes: &[u8],
    case: &str,
    expected: &str,
) -> PipelineFailure {
    let path = TestPath::write("pdf", bytes);
    let run = ingest_fixture(conn, &path.0);
    let error = parse_document(conn, &PdfExtractParser::new(), &run.run_id).expect_err(case);
    assert_eq!(error.code(), expected, "{case}");
    let failed = db::get_pipeline_run(conn, &run.run_id).unwrap().unwrap();
    assert_eq!(failed.state, PipelineState::Failed, "{case}");
    let failure = failed.failure.unwrap();
    assert_eq!(failure.code, expected, "{case}");
    assert_eq!(failure.stage, Some(PipelineStage::Parse), "{case}");
    assert!(!failure.recoverable, "{case}");
    assert!(
        db::get_parsed_document(conn, &run.run_id)
            .unwrap()
            .is_none(),
        "{case}"
    );
    assert!(
        db::get_ocr_handoff_for_root(conn, &run.run_id)
            .unwrap()
            .is_none(),
        "{case}"
    );
    failure
}

fn pdf_bytes(mut pdf: PdfDocument) -> Vec<u8> {
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn checked_encryption_uses_identity_defaults_and_refuses_an_unknown_filter() {
    let mut pdf = build_pdf(vec![FixturePage::Text(b"Identity stream".to_vec())]);
    set_file_ids(&mut pdf);
    let filter: Arc<dyn CryptFilter> = Arc::new(IdentityCryptFilter);
    let state = EncryptionState::try_from(EncryptionVersion::V4 {
        document: &pdf,
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"Identity".to_vec(), filter)]),
        stream_filter: b"Identity".to_vec(),
        string_filter: b"Identity".to_vec(),
        owner_password: "owner-password",
        user_password: "",
        permissions: Permissions::PRINTABLE,
    })
    .unwrap();
    pdf.encrypt(&state).unwrap();
    let id = pdf.trailer.get(b"Encrypt").unwrap().as_reference().unwrap();
    let dictionary = pdf.get_dictionary_mut(id).unwrap();
    dictionary.remove(b"StmF");
    dictionary.remove(b"StrF");
    dictionary.remove(b"CF");

    let identity = pdf_bytes(pdf.clone());
    let doc = PdfDocument::load_mem_checked_encryption(&identity).unwrap();
    let mut text = String::new();
    output_doc_page(&doc, &mut PlainTextOutput::new(&mut text), 1).unwrap();
    assert_eq!(text.trim(), "Identity stream");

    pdf.get_dictionary_mut(id).unwrap().set(
        "CF",
        dictionary! { "StdCF" => dictionary! { "CFM" => "UnimplementedCipher" } },
    );
    pdf.get_dictionary_mut(id).unwrap().set("StmF", "StdCF");
    let mut conn = db::init_db(":memory:").unwrap();
    assert_parse_refusal(
        &mut conn,
        &pdf_bytes(pdf),
        "unknown stream cipher",
        "PDF_ENCRYPTION_UNSUPPORTED",
    );
}

#[test]
fn checked_stream_decryption_consumes_crypt_filter_and_decrypts_dictionary_strings() {
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    let mut pdf = build_pdf(vec![FixturePage::Text(b"Encrypted Flate page".to_vec())]);
    let page = pdf.get_pages()[&1];
    let stream_id = pdf.get_page_contents(page)[0];
    let stream = pdf
        .objects
        .get_mut(&stream_id)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    let clear = stream.content.clone();
    let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&stream.content).unwrap();
    stream.set_content(encoder.finish().unwrap());
    stream.dict.set(
        "Filter",
        vec![
            Object::Name(b"Crypt".to_vec()),
            Object::Name(b"FlateDecode".to_vec()),
        ],
    );
    stream.dict.set(
        "DecodeParms",
        vec![
            Object::Dictionary(dictionary! { "Name" => "StdCF" }),
            Object::Null,
        ],
    );
    let state = encrypt_aes(&mut pdf);
    let mut marker = Object::string_literal("synthetic dictionary string");
    lopdf::encryption::encrypt_object(&state, stream_id, &mut marker).unwrap();
    pdf.objects
        .get_mut(&stream_id)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("Marker", marker);

    let opened = PdfDocument::load_mem_checked_encryption(&pdf_bytes(pdf)).unwrap();
    let stream = opened.get_object(stream_id).unwrap().as_stream().unwrap();
    assert!(!stream.is_compressed());
    assert_eq!(stream.content, clear);
    assert_eq!(
        stream.dict.get(b"Marker").unwrap().as_str().unwrap(),
        b"synthetic dictionary string"
    );
    let mut text = String::new();
    output_doc_page(&opened, &mut PlainTextOutput::new(&mut text), 1).unwrap();
    assert_eq!(text.trim(), "Encrypted Flate page");
}
