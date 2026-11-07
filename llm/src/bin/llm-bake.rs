//! `llm-bake`: appends a pre-computed prompt cache to an llmos model file.
//!
//! ```text
//! cargo run --release -p llm --bin llm-bake -- [models/llmos.model]
//! ```
//!
//! Runs [`llm::agent::static_prompt`] through the model and writes the
//! resulting key/value cache after the weights (replacing any previous one),
//! so the kernel can skip that work at boot. See [`llm::prompt_cache`].

use std::time::Instant;

use llm::{AlignedBuffer, Model, prompt_cache};

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "models/llmos.model".into());
    let mut bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    let offset = prompt_cache::section_offset(&bytes).expect("valid model file");
    bytes.truncate(offset.min(bytes.len())); // Drop a previously baked cache.

    let buf = AlignedBuffer::from_bytes(&bytes);
    let model = Model::from_bytes(buf.as_bytes()).expect("valid model file");
    let tokens = model.tokenizer.encode(&llm::agent::static_prompt());
    println!(
        "baking {} prompt tokens for {}...",
        tokens.len(),
        model.name
    );
    let start = Instant::now();
    let section = prompt_cache::bake(&model, buf.as_bytes(), &tokens);
    println!("done in {:.1?}", start.elapsed());

    bytes.resize(offset, 0);
    bytes.extend_from_slice(&section);
    std::fs::write(&path, &bytes).unwrap_or_else(|e| panic!("cannot write {path}: {e}"));
    println!(
        "wrote {path}: +{:.1} MiB prompt cache",
        section.len() as f64 / (1024.0 * 1024.0)
    );
}
