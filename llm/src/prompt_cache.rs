//! Pre-computed key/value cache for a fixed prompt prefix ("prompt caching").
//!
//! Every llmos prompt starts with the same instructions and examples
//! ([`crate::agent::static_prompt`]). Running those ~450 tokens through the
//! model takes ~20 s on a real CPU but many minutes under software emulation.
//! Because the computation is deterministic, it can be done once on a fast
//! host (`cargo run --release -p llm --bin llm-bake`) and the resulting
//! key/value cache appended to the model file. At boot the kernel copies the
//! cache into its [`State`] and only processes the machine-specific rest.
//!
//! The cache is an optional section placed at the first 64-byte boundary
//! after the weights:
//!
//! ```text
//! u8[8]  magic "LLMOSKVC"
//! u32    version (1)
//! u32    n_tokens
//! u32    n_layers          (must match the model)
//! u32    kv_dim            (must match the model)
//! u64    fingerprint       (FNV-1a of the model header; detects a mismatch)
//! u32[n_tokens]  token ids of the cached prefix
//! -- pad to 64 --
//! f32[n_layers × n_tokens × kv_dim]  keys    (layer-major)
//! f32[n_layers × n_tokens × kv_dim]  values
//! ```

use alloc::vec::Vec;

use crate::format::{self, FormatError, HEADER_SIZE, Reader, TENSOR_ALIGN};
use crate::model::{Model, State};
use crate::tokenizer::TokenId;

/// Section signature.
pub const MAGIC: &[u8; 8] = b"LLMOSKVC";
/// Section version this crate reads and writes.
pub const VERSION: u32 = 1;

/// FNV-1a hash of the model header: ties a cache to one exact model file.
pub fn fingerprint(model_file: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in &model_file[..HEADER_SIZE.min(model_file.len())] {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Byte offset where the cache section starts (or would start) in a model
/// file: the first aligned position after the weights.
///
/// # Errors
///
/// Fails if the header is invalid.
pub fn section_offset(model_file: &[u8]) -> Result<usize, FormatError> {
    let h = format::parse_header(model_file)?;
    Ok((h.weights_offset + h.weights_size).div_ceil(TENSOR_ALIGN) * TENSOR_ALIGN)
}

/// A prompt cache borrowed from a model file.
pub struct PromptCache<'a> {
    tokens: Vec<TokenId>,
    keys: &'a [f32],
    values: &'a [f32],
}

impl<'a> PromptCache<'a> {
    /// Looks for a cache section in `model_file` that belongs to `model`.
    /// Returns `None` if there is none or it does not match.
    pub fn find(model_file: &'a [u8], model: &Model<'_>) -> Option<Self> {
        let offset = section_offset(model_file).ok()?;
        let section = model_file.get(offset..)?;
        let mut r = Reader::new(section);
        if r.bytes(8).ok()? != MAGIC || r.u32().ok()? != VERSION {
            return None;
        }
        let n_tokens = r.u32().ok()? as usize;
        let (n_layers, kv_dim) = (r.u32().ok()? as usize, r.u32().ok()? as usize);
        let c = &model.config;
        if n_layers != c.n_layers
            || kv_dim != c.kv_dim()
            || r.u64().ok()? != fingerprint(model_file)
        {
            return None;
        }
        let mut tokens = Vec::with_capacity(n_tokens);
        for _ in 0..n_tokens {
            tokens.push(r.u32().ok()?);
        }
        r.align_to(TENSOR_ALIGN);
        let n = n_layers * n_tokens * kv_dim;
        let start = offset + r.position();
        let keys = cast_f32(model_file.get(start..start + n * 4)?)?;
        let values = cast_f32(model_file.get(start + n * 4..start + n * 8)?)?;
        Some(Self {
            tokens,
            keys,
            values,
        })
    }

    /// Tokens covered by the cache.
    pub fn tokens(&self) -> &[TokenId] {
        &self.tokens
    }

    /// If the cache covers a prefix of `prompt`, loads it into `state`
    /// (which must be fresh or rewound to 0) and returns how many tokens of
    /// `prompt` are now processed. Returns 0 if the cache does not apply.
    pub fn apply(&self, state: &mut State, prompt: &[TokenId]) -> usize {
        let n = self.tokens.len();
        if n == 0 || n > prompt.len() || prompt[..n] != self.tokens[..] || n > state.seq_len {
            return 0;
        }
        state.import_cache(n, self.keys, self.values);
        n
    }
}

fn cast_f32(bytes: &[u8]) -> Option<&[f32]> {
    if bytes.as_ptr() as usize % 4 != 0 {
        return None;
    }
    // SAFETY: alignment checked above; every bit pattern is a valid f32 and
    // the file is little-endian like every supported target.
    Some(unsafe { core::slice::from_raw_parts(bytes.as_ptr() as *const f32, bytes.len() / 4) })
}

/// Runs `prompt` through `model` and serialises the resulting cache section.
///
/// `model_file` must be the bytes `model` was loaded from (for the
/// fingerprint). Append the result at [`section_offset`].
pub fn bake(model: &Model<'_>, model_file: &[u8], prompt: &[TokenId]) -> Vec<u8> {
    let c = &model.config;
    let mut state = model.new_state(prompt.len());
    for &t in prompt {
        model.forward(&mut state, t, crate::Logits::None);
    }
    let (keys, values) = state.export_cache(prompt.len());

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(prompt.len() as u32).to_le_bytes());
    out.extend_from_slice(&(c.n_layers as u32).to_le_bytes());
    out.extend_from_slice(&(c.kv_dim() as u32).to_le_bytes());
    out.extend_from_slice(&fingerprint(model_file).to_le_bytes());
    for &t in prompt {
        out.extend_from_slice(&t.to_le_bytes());
    }
    out.resize(out.len().div_ceil(TENSOR_ALIGN) * TENSOR_ALIGN, 0);
    for v in keys.iter().chain(values.iter()) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}
