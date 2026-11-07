//! Llama-architecture transformer inference.
//!
//! Implements the forward pass of decoder-only models such as Llama, SmolLM2
//! and Qwen2 (without biases):
//!
//! ```text
//! x = embed(token)
//! for each layer:
//!     h = rmsnorm(x);  q,k,v = Wq h, Wk h, Wv h;  rope(q, k)
//!     x += Wo · attention(q, k_cache, v_cache)          (grouped-query)
//!     h = rmsnorm(x);  x += W2 · (silu(W1 h) ⊙ W3 h)    (SwiGLU)
//! logits = Wcls · rmsnorm(x)
//! ```
//!
//! Rotary embeddings use the Hugging Face "rotate half" layout (pairs
//! `(i, i + head_dim/2)`), so exported checkpoints need no permutation.
//!
//! Weights are borrowed from the model file; only the activations and the
//! key/value cache ([`State`]) are allocated.

use alloc::vec;
use alloc::vec::Vec;

use crate::format::{self, Config, FormatError, Reader, TENSOR_ALIGN, WeightType};
use crate::math;
use crate::tokenizer::{TokenId, Tokenizer};

/// A weight matrix (or vector) borrowed from the model file.
#[derive(Clone, Copy)]
pub enum Tensor<'a> {
    /// Plain 32-bit floats.
    F32(&'a [f32]),
    /// Q8_0: int8 values with one f32 scale per group.
    Q8 {
        /// Quantised values.
        values: &'a [i8],
        /// One scale per `group_size` values.
        scales: &'a [f32],
    },
}

/// All weights of one transformer block.
pub struct Layer<'a> {
    /// RMSNorm weight before attention.
    pub attn_norm: &'a [f32],
    /// Query projection, `[dim, dim]`.
    pub wq: Tensor<'a>,
    /// Key projection, `[kv_dim, dim]`.
    pub wk: Tensor<'a>,
    /// Value projection, `[kv_dim, dim]`.
    pub wv: Tensor<'a>,
    /// Attention output projection, `[dim, dim]`.
    pub wo: Tensor<'a>,
    /// RMSNorm weight before the feed-forward network.
    pub ffn_norm: &'a [f32],
    /// Gate projection, `[hidden_dim, dim]`.
    pub w1: Tensor<'a>,
    /// Down projection, `[dim, hidden_dim]`.
    pub w2: Tensor<'a>,
    /// Up projection, `[hidden_dim, dim]`.
    pub w3: Tensor<'a>,
}

/// A loaded model: configuration, tokenizer and weights.
pub struct Model<'a> {
    /// Hyper-parameters.
    pub config: Config,
    /// Model name from the file header.
    pub name: &'a str,
    /// The tokenizer.
    pub tokenizer: Tokenizer<'a>,
    /// Token embedding table, `[vocab_size, dim]`.
    pub token_embedding: Tensor<'a>,
    /// Transformer blocks.
    pub layers: Vec<Layer<'a>>,
    /// Final RMSNorm weight.
    pub final_norm: &'a [f32],
    /// Output classifier, `[vocab_size, dim]` (the embedding when tied).
    pub classifier: Tensor<'a>,
    /// Inverse rotary frequencies, `[head_dim / 2]`.
    inv_freq: Vec<f32>,
}

/// Reads `n` aligned little-endian `T`s in place.
fn cast_slice<'a, T>(bytes: &'a [u8], n: usize) -> Result<&'a [T], FormatError> {
    let len = n * core::mem::size_of::<T>();
    if bytes.len() < len {
        return Err(FormatError::Truncated);
    }
    if bytes.as_ptr() as usize % core::mem::align_of::<T>() != 0 {
        return Err(FormatError::Misaligned);
    }
    // SAFETY: length and alignment were checked above; `T` is only ever `f32`
    // or `i8`, for which every bit pattern is valid. The file format is
    // little-endian, as are all targets llmos supports.
    Ok(unsafe { core::slice::from_raw_parts(bytes.as_ptr() as *const T, n) })
}

/// Reads tensors sequentially from the weights section.
struct TensorReader<'a> {
    file: &'a [u8],
    r: Reader<'a>,
    weight_type: WeightType,
    group_size: usize,
}

impl<'a> TensorReader<'a> {
    fn aligned(&mut self, n_bytes: usize) -> Result<&'a [u8], FormatError> {
        self.r.align_to(TENSOR_ALIGN);
        let start = self.r.position();
        self.r.bytes(n_bytes)?;
        Ok(&self.file[start..start + n_bytes])
    }

    /// A vector that is always stored as f32 (norm weights).
    fn f32_vec(&mut self, n: usize) -> Result<&'a [f32], FormatError> {
        let bytes = self.aligned(n * 4)?;
        cast_slice(bytes, n)
    }

    /// A weight matrix stored in the file's weight type.
    fn matrix(&mut self, rows: usize, cols: usize) -> Result<Tensor<'a>, FormatError> {
        let n = rows * cols;
        Ok(match self.weight_type {
            WeightType::F32 => Tensor::F32(self.f32_vec(n)?),
            WeightType::Q8 => {
                let values = cast_slice(self.aligned(n)?, n)?;
                let scales = self.f32_vec(n / self.group_size)?;
                Tensor::Q8 { values, scales }
            }
        })
    }
}

impl<'a> Model<'a> {
    /// Loads a model from the bytes of a `.model` file.
    ///
    /// `file` must start at a 64-byte aligned address (true for RAM disks,
    /// which are page aligned, and for [`AlignedBuffer`](crate::AlignedBuffer)).
    ///
    /// # Errors
    ///
    /// Returns a [`FormatError`] if the file is malformed or misaligned.
    pub fn from_bytes(file: &'a [u8]) -> Result<Self, FormatError> {
        if file.as_ptr() as usize % TENSOR_ALIGN != 0 {
            return Err(FormatError::Misaligned);
        }
        let header = format::parse_header(file)?;
        let c = header.config;
        let tok_bytes =
            &file[header.tokenizer_offset..header.tokenizer_offset + header.tokenizer_size];
        let tokenizer = Tokenizer::from_bytes(tok_bytes)?;
        if tokenizer.vocab_size() != c.vocab_size {
            return Err(FormatError::BadTokenizer(
                "vocabulary size does not match the header",
            ));
        }

        let weights = &file[..header.weights_offset + header.weights_size];
        let mut r = Reader::new(weights);
        r.bytes(header.weights_offset)?;
        let mut t = TensorReader {
            file: weights,
            r,
            weight_type: c.weight_type,
            group_size: c.group_size,
        };

        let (dim, hidden, kv_dim) = (c.dim, c.hidden_dim, c.kv_dim());
        let token_embedding = t.matrix(c.vocab_size, dim)?;
        let mut layers = Vec::with_capacity(c.n_layers);
        for _ in 0..c.n_layers {
            layers.push(Layer {
                attn_norm: t.f32_vec(dim)?,
                wq: t.matrix(dim, dim)?,
                wk: t.matrix(kv_dim, dim)?,
                wv: t.matrix(kv_dim, dim)?,
                wo: t.matrix(dim, dim)?,
                ffn_norm: t.f32_vec(dim)?,
                w1: t.matrix(hidden, dim)?,
                w2: t.matrix(dim, hidden)?,
                w3: t.matrix(hidden, dim)?,
            });
        }
        let final_norm = t.f32_vec(dim)?;
        let classifier = if c.tied_embeddings {
            token_embedding
        } else {
            t.matrix(c.vocab_size, dim)?
        };

        let half = c.head_dim() / 2;
        let inv_freq = (0..half)
            .map(|i| 1.0 / math::powf(c.rope_theta, (2 * i) as f32 / c.head_dim() as f32))
            .collect();

        // The name lives in the header; borrow it from the file for 'a.
        let name_bytes = &file[104..128];
        let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(24);
        let name = core::str::from_utf8(&name_bytes[..end]).unwrap_or("?");

        Ok(Self {
            config: c,
            name,
            tokenizer,
            token_embedding,
            layers,
            final_norm,
            classifier,
            inv_freq,
        })
    }

    /// Creates inference state for contexts of up to `seq_len` tokens
    /// (clamped to the model's maximum).
    pub fn new_state(&self, seq_len: usize) -> State {
        State::new(&self.config, seq_len.min(self.config.max_seq_len))
    }

    /// Bytes of RAM a [`State`] with `seq_len` positions will allocate.
    pub fn state_bytes(&self, seq_len: usize) -> usize {
        let c = &self.config;
        let kv = 2 * c.n_layers * seq_len * c.kv_dim();
        let acts =
            4 * c.dim + 2 * c.hidden_dim + 2 * c.kv_dim() + c.n_heads * seq_len + c.vocab_size;
        (kv + acts) * 4 + c.dim.max(c.hidden_dim) * 2
    }

    /// Runs the model on `token` at position `state.pos`, appends its keys
    /// and values to the cache and advances `state.pos`.
    ///
    /// `logits` selects which output scores are computed; skipping them
    /// (prompt tokens) or restricting them (constrained decoding) saves the
    /// cost of the large classifier matrix. The returned slice is indexed by
    /// token id; entries that were not requested are unspecified.
    ///
    /// # Panics
    ///
    /// Panics if the context is full (`state.pos == state.seq_len`).
    pub fn forward<'s>(
        &self,
        state: &'s mut State,
        token: TokenId,
        logits: Logits<'_>,
    ) -> &'s [f32] {
        let c = &self.config;
        let pos = state.pos;
        assert!(pos < state.seq_len, "context window is full");
        let (dim, kv_dim, head_dim) = (c.dim, c.kv_dim(), c.head_dim());
        let kv_mul = c.n_heads / c.n_kv_heads;
        let gs = c.group_size;

        embedding_row(&self.token_embedding, token as usize, dim, gs, &mut state.x);

        for (l, layer) in self.layers.iter().enumerate() {
            // ---- Attention block ----
            math::rmsnorm(&mut state.xb, &state.x, layer.attn_norm, c.norm_eps);
            state.xq.quantize(&state.xb, gs);
            let cache_off = (l * state.seq_len + pos) * kv_dim;
            matmul(&mut state.q, &layer.wq, &state.xb, &state.xq, gs);
            matmul(
                &mut state.key_cache[cache_off..cache_off + kv_dim],
                &layer.wk,
                &state.xb,
                &state.xq,
                gs,
            );
            matmul(
                &mut state.value_cache[cache_off..cache_off + kv_dim],
                &layer.wv,
                &state.xb,
                &state.xq,
                gs,
            );

            rope(&mut state.q, head_dim, pos, &self.inv_freq);
            rope(
                &mut state.key_cache[cache_off..cache_off + kv_dim],
                head_dim,
                pos,
                &self.inv_freq,
            );

            let scale = 1.0 / math::sqrtf(head_dim as f32);
            let layer_base = l * state.seq_len * kv_dim;
            for h in 0..c.n_heads {
                let q = &state.q[h * head_dim..(h + 1) * head_dim];
                let att = &mut state.att[h * state.seq_len..h * state.seq_len + pos + 1];
                let kv_head = (h / kv_mul) * head_dim;
                for (t, score) in att.iter_mut().enumerate() {
                    let k = &state.key_cache[layer_base + t * kv_dim + kv_head..][..head_dim];
                    *score = math::dot(q, k) * scale;
                }
                math::softmax(att);
                let out = &mut state.xb[h * head_dim..(h + 1) * head_dim];
                out.fill(0.0);
                for (t, &a) in att.iter().enumerate() {
                    let v = &state.value_cache[layer_base + t * kv_dim + kv_head..][..head_dim];
                    math::axpy(out, a, v);
                }
            }
            state.xq.quantize(&state.xb, gs);
            matmul(&mut state.xb2, &layer.wo, &state.xb, &state.xq, gs);
            math::add_assign(&mut state.x, &state.xb2);

            // ---- Feed-forward block (SwiGLU) ----
            math::rmsnorm(&mut state.xb, &state.x, layer.ffn_norm, c.norm_eps);
            state.xq.quantize(&state.xb, gs);
            matmul(&mut state.hb, &layer.w1, &state.xb, &state.xq, gs);
            matmul(&mut state.hb2, &layer.w3, &state.xb, &state.xq, gs);
            for (g, &u) in state.hb.iter_mut().zip(&state.hb2) {
                *g = math::silu(*g) * u;
            }
            state.hq.quantize(&state.hb, gs);
            matmul(&mut state.xb, &layer.w2, &state.hb, &state.hq, gs);
            math::add_assign(&mut state.x, &state.xb);
        }

        math::rmsnorm_in_place(&mut state.x, self.final_norm, c.norm_eps);
        match logits {
            Logits::None => {}
            Logits::All => {
                state.xq.quantize(&state.x, gs);
                matmul(&mut state.logits, &self.classifier, &state.x, &state.xq, gs);
            }
            Logits::Subset(ids) => {
                state.xq.quantize(&state.x, gs);
                for &id in ids {
                    state.logits[id as usize] =
                        row_dot(&self.classifier, id as usize, &state.x, &state.xq, gs);
                }
            }
        }
        state.pos += 1;
        &state.logits
    }
}

/// Which output logits [`Model::forward`] should compute.
#[derive(Debug, Clone, Copy)]
pub enum Logits<'a> {
    /// None (prompt processing).
    None,
    /// The full vocabulary.
    All,
    /// Only these token ids.
    Subset(&'a [TokenId]),
}

/// An activation vector quantised to int8 in groups (for Q8 matmuls).
#[derive(Debug, Clone, Default)]
pub struct QuantizedVec {
    values: Vec<i8>,
    scales: Vec<f32>,
}

impl QuantizedVec {
    fn with_len(n: usize) -> Self {
        Self {
            values: vec![0; n],
            scales: vec![0.0; n],
        }
    }

    /// Symmetric per-group quantisation: `q = round(x / scale)`,
    /// `scale = max|x| / 127`.
    fn quantize(&mut self, x: &[f32], group_size: usize) {
        if group_size == 0 {
            return; // F32 model: unused.
        }
        for (g, chunk) in x.chunks(group_size).enumerate() {
            let max = chunk.iter().fold(0.0f32, |m, v| m.max(math::fabsf(*v)));
            let scale = max / 127.0;
            let inv = if scale > 0.0 { 1.0 / scale } else { 0.0 };
            self.scales[g] = scale;
            for (q, v) in self.values[g * group_size..].iter_mut().zip(chunk) {
                *q = math::roundf(v * inv) as i8;
            }
        }
    }
}

/// Mutable inference state: activations and the key/value cache.
pub struct State {
    /// Next position to be written (number of tokens processed).
    pub pos: usize,
    /// Capacity of the key/value cache, in tokens.
    pub seq_len: usize,
    n_layers: usize,
    x: Vec<f32>,
    xb: Vec<f32>,
    xb2: Vec<f32>,
    hb: Vec<f32>,
    hb2: Vec<f32>,
    q: Vec<f32>,
    xq: QuantizedVec,
    hq: QuantizedVec,
    att: Vec<f32>,
    logits: Vec<f32>,
    key_cache: Vec<f32>,
    value_cache: Vec<f32>,
}

impl State {
    fn new(c: &Config, seq_len: usize) -> Self {
        let kv_cache = c.n_layers * seq_len * c.kv_dim();
        Self {
            pos: 0,
            seq_len,
            n_layers: c.n_layers,
            x: vec![0.0; c.dim],
            xb: vec![0.0; c.dim],
            xb2: vec![0.0; c.dim],
            hb: vec![0.0; c.hidden_dim],
            hb2: vec![0.0; c.hidden_dim],
            q: vec![0.0; c.dim],
            xq: QuantizedVec::with_len(c.dim),
            hq: QuantizedVec::with_len(c.hidden_dim),
            att: vec![0.0; c.n_heads * seq_len],
            logits: vec![0.0; c.vocab_size],
            key_cache: vec![0.0; kv_cache],
            value_cache: vec![0.0; kv_cache],
        }
    }

    /// Logits produced by the most recent [`Model::forward`] call.
    pub fn logits(&self) -> &[f32] {
        &self.logits
    }

    /// Copies the keys and values of the first `n` positions out of the
    /// cache, layer-major (`[n_layers][n][kv_dim]`). Used to bake a
    /// [`PromptCache`](crate::prompt_cache::PromptCache).
    pub fn export_cache(&self, n: usize) -> (Vec<f32>, Vec<f32>) {
        let n = n.min(self.pos);
        let kv_dim = self.key_cache.len() / self.n_layers / self.seq_len;
        let (mut keys, mut values) = (Vec::new(), Vec::new());
        for l in 0..self.n_layers {
            let base = l * self.seq_len * kv_dim;
            keys.extend_from_slice(&self.key_cache[base..base + n * kv_dim]);
            values.extend_from_slice(&self.value_cache[base..base + n * kv_dim]);
        }
        (keys, values)
    }

    /// Loads `n` positions of keys and values (layout as produced by
    /// [`export_cache`](Self::export_cache)) and sets `pos = n`.
    ///
    /// # Panics
    ///
    /// Panics if `n` exceeds the context size or the slices are too short.
    pub fn import_cache(&mut self, n: usize, keys: &[f32], values: &[f32]) {
        assert!(n <= self.seq_len, "cache larger than the context window");
        let kv_dim = self.key_cache.len() / self.n_layers / self.seq_len;
        for l in 0..self.n_layers {
            let (src, dst) = (l * n * kv_dim, l * self.seq_len * kv_dim);
            self.key_cache[dst..dst + n * kv_dim].copy_from_slice(&keys[src..src + n * kv_dim]);
            self.value_cache[dst..dst + n * kv_dim].copy_from_slice(&values[src..src + n * kv_dim]);
        }
        self.pos = n;
    }

    /// Rewinds the context to `pos` tokens. Cached keys/values before `pos`
    /// are kept, so a shared prompt prefix is only processed once.
    pub fn rewind(&mut self, pos: usize) {
        self.pos = pos.min(self.pos);
    }
}

/// Copies (dequantising if needed) row `row` of an embedding table.
fn embedding_row(t: &Tensor<'_>, row: usize, dim: usize, gs: usize, out: &mut [f32]) {
    match t {
        Tensor::F32(w) => out.copy_from_slice(&w[row * dim..(row + 1) * dim]),
        Tensor::Q8 { values, scales } => {
            for (i, o) in out.iter_mut().enumerate() {
                let idx = row * dim + i;
                *o = values[idx] as f32 * scales[idx / gs];
            }
        }
    }
}

/// `out = W x` for a `[out.len(), x.len()]` matrix.
fn matmul(out: &mut [f32], w: &Tensor<'_>, x: &[f32], xq: &QuantizedVec, gs: usize) {
    for (i, o) in out.iter_mut().enumerate() {
        *o = row_dot(w, i, x, xq, gs);
    }
}

/// Dot product of matrix row `row` with `x`.
#[inline]
fn row_dot(w: &Tensor<'_>, row: usize, x: &[f32], xq: &QuantizedVec, gs: usize) -> f32 {
    let n = x.len();
    match w {
        Tensor::F32(w) => math::dot(&w[row * n..(row + 1) * n], x),
        Tensor::Q8 { values, scales } => {
            let wv = &values[row * n..(row + 1) * n];
            let ws = &scales[row * n / gs..(row + 1) * n / gs];
            let mut sum = 0.0f32;
            for (g, (wg, xg)) in wv
                .chunks_exact(gs)
                .zip(xq.values.chunks_exact(gs))
                .enumerate()
            {
                sum += math::dot_i8(wg, xg) as f32 * ws[g] * xq.scales[g];
            }
            sum
        }
    }
}

/// Applies rotary position embeddings ("rotate half" layout) to every head
/// in `v` at position `pos`.
fn rope(v: &mut [f32], head_dim: usize, pos: usize, inv_freq: &[f32]) {
    let half = head_dim / 2;
    for (i, &f) in inv_freq.iter().enumerate() {
        let angle = pos as f32 * f;
        let (sin, cos) = (math::sinf(angle), math::cosf(angle));
        for head in v.chunks_exact_mut(head_dim) {
            let (a, b) = (head[i], head[i + half]);
            head[i] = a * cos - b * sin;
            head[i + half] = b * cos + a * sin;
        }
    }
}
