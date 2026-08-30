use pdf_extract::*;
use std::fs::File;
use std::io::Write;

fn main() {
    let path = "test_min.pdf";
    let minimal_pdf = b"%PDF-1.4\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 4 0 R >> >> /MediaBox [0 0 300 144] /Contents 5 0 R >>\nendobj\n\
4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n\
5 0 obj\n<< /Length 44 >>\nstream\n\
BT\n/F1 12 Tf\n72 72 Td\n(Hello World) Tj\nET\n\
endstream\nendobj\n\
xref\n0 6\n0000000000 65535 f \n0000000009 00000 n \n0000000058 00000 n \n0000000115 00000 n \n0000000249 00000 n \n0000000342 00000 n \ntrailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n436\n%%EOF";

    let mut f = File::create(path).unwrap();
    f.write_all(minimal_pdf).unwrap();

    let texts = extract_text_by_pages(path).unwrap();
    println!("Pages: {:?}", texts);
}
