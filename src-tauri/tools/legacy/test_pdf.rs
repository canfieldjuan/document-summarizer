use pdf_extract::*;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Provide pdf path");
        return;
    }
    let path = &args[1];

    // Check if we can get pages
    let bytes = std::fs::read(path).unwrap();
    let out = extract_text_from_mem(&bytes).unwrap();
    println!("Text: {}", out);
}
