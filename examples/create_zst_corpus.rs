// Helper script: create .zst test corpus files
// Run with: cargo run --example create_zst_corpus
use std::fs::File;
use std::io::{BufReader, BufWriter};

fn main() {
    let pairs = [
        (
            "test_corpus/multiple_tables.sql",
            "test_corpus/multiple_tables.sql.zst",
        ),
        (
            "test_corpus/null_representations.sql",
            "test_corpus/null_representations.sql.zst",
        ),
    ];

    for (src, dst) in &pairs {
        let input = File::open(src).unwrap_or_else(|e| panic!("Cannot open {}: {}", src, e));
        let output = File::create(dst).unwrap_or_else(|e| panic!("Cannot create {}: {}", dst, e));
        let mut encoder =
            zstd::Encoder::new(BufWriter::new(output), 3).expect("zstd encoder failed");
        std::io::copy(&mut BufReader::new(input), &mut encoder).expect("copy failed");
        encoder.finish().expect("finish failed");
        println!("Created {}", dst);
    }
}
