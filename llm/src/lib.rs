//! # llm: a tiny `no_std` inference engine
//!
//! Runs Llama-architecture language models (SmolLM2, TinyLlama, Llama, ...)
//! on bare metal. The crate needs only `core` and `alloc`, so the same code
//! runs inside the llmos kernel and in ordinary host tests.
//!
//! * [`format`]: the `.model` file layout (header, tokenizer, weights).
//! * [`tokenizer`]: byte-level BPE compatible with Hugging Face `tokenizers`.
//! * [`model`]: the transformer forward pass with f32 or Q8 weights.
//! * [`constrained`]: grammar-constrained decoding of JSON actions.
//! * [`agent`]: the shell-agent protocol (action list and prompt).
//! * [`prompt_cache`]: pre-computed key/value cache for the fixed prompt.
//! * [`generate`]: plain greedy text generation.
//!
//! ```no_run
//! # let bytes = llm::AlignedBuffer::from_bytes(&[]);
//! let model = llm::Model::from_bytes(bytes.as_bytes()).expect("valid model");
//! let mut state = model.new_state(512);
//! let prompt = model.tokenizer.encode("Once upon a time");
//! let text = llm::generate::greedy(&model, &mut state, &prompt, 20, &[]);
//! ```

#![no_std]
#![warn(missing_docs)]

extern crate alloc;

pub mod agent;
pub mod constrained;
pub mod format;
pub mod generate;
pub mod math;
pub mod model;
pub mod prompt_cache;
pub mod tokenizer;

pub use format::{Config, FormatError, WeightType};
pub use model::{Logits, Model, State};
pub use tokenizer::{TokenId, Tokenizer};

use alloc::vec::Vec;

/// A byte buffer whose start is 64-byte aligned, as [`Model::from_bytes`]
/// requires. Useful on hosts, where a plain `Vec<u8>` has no such guarantee.
pub struct AlignedBuffer {
    words: Vec<Block>,
    len: usize,
}

/// One 64-byte, 64-byte-aligned chunk of an [`AlignedBuffer`].
#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct Block([u8; 64]);

impl AlignedBuffer {
    /// Copies `bytes` into a fresh aligned buffer.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut words = alloc::vec![Block([0; 64]); bytes.len().div_ceil(64)];
        // SAFETY: `words` owns at least `bytes.len()` initialised bytes.
        let dst = unsafe {
            core::slice::from_raw_parts_mut(words.as_mut_ptr() as *mut u8, words.len() * 64)
        };
        dst[..bytes.len()].copy_from_slice(bytes);
        Self {
            words,
            len: bytes.len(),
        }
    }

    /// The buffer contents.
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: the first `len` bytes were initialised in `from_bytes`.
        unsafe { core::slice::from_raw_parts(self.words.as_ptr() as *const u8, self.len) }
    }
}
