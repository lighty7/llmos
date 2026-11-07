//! The llmos model file format (`.model`).
//!
//! One self-contained little-endian file holding the model configuration,
//! the tokenizer and all weights, designed to be used *in place*: the kernel
//! receives it as a RAM disk and the weight tensors are borrowed directly
//! from it, without copying. Every tensor therefore starts at a 64-byte
//! aligned offset.
//!
//! Files are produced by `tools/export_model.py`; the layout is documented in
//! `docs/model-format.md` and mirrored by [`Header`].
//!
//! ```text
//! [ header (128 bytes) ][ tokenizer section ][ weights section ]
//! ```

use core::fmt;

/// File signature.
pub const MAGIC: &[u8; 8] = b"LLMOSMDL";
/// Format version this crate reads.
pub const FORMAT_VERSION: u32 = 1;
/// Size of the fixed header.
pub const HEADER_SIZE: usize = 128;
/// Alignment of every tensor in the weights section.
pub const TENSOR_ALIGN: usize = 64;

/// How weight matrices are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightType {
    /// 32-bit floats.
    F32,
    /// 8-bit integers with one f32 scale per group of `group_size` values
    /// ("Q8_0"): `w = q * scale`.
    Q8,
}

/// Model hyper-parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// Width of the residual stream (embedding size).
    pub dim: usize,
    /// Inner size of the feed-forward network.
    pub hidden_dim: usize,
    /// Number of transformer blocks.
    pub n_layers: usize,
    /// Number of query heads.
    pub n_heads: usize,
    /// Number of key/value heads (grouped-query attention when < `n_heads`).
    pub n_kv_heads: usize,
    /// Number of tokens in the vocabulary.
    pub vocab_size: usize,
    /// Longest context the model was trained for.
    pub max_seq_len: usize,
    /// Base of the rotary position embedding frequencies.
    pub rope_theta: f32,
    /// Epsilon of RMS normalisation.
    pub norm_eps: f32,
    /// Weight storage format.
    pub weight_type: WeightType,
    /// Quantisation group size (Q8 only).
    pub group_size: usize,
    /// Whether the output classifier reuses the token embedding matrix.
    pub tied_embeddings: bool,
    /// Beginning-of-sequence token id.
    pub bos_id: u32,
    /// End-of-turn token id (generation stops here).
    pub eos_id: u32,
}

impl Config {
    /// Size of one attention head.
    pub fn head_dim(&self) -> usize {
        self.dim / self.n_heads
    }

    /// Total width of the keys/values of one position.
    pub fn kv_dim(&self) -> usize {
        self.n_kv_heads * self.head_dim()
    }

    /// Approximate parameter count.
    pub fn parameter_count(&self) -> usize {
        let (d, h, kv) = (self.dim, self.hidden_dim, self.kv_dim());
        let per_layer = 2 * d * d + 2 * d * kv + 3 * d * h + 2 * d;
        let classifier = if self.tied_embeddings {
            0
        } else {
            self.vocab_size * d
        };
        self.vocab_size * d + self.n_layers * per_layer + d + classifier
    }
}

/// Errors detected while reading a model file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatError {
    /// The file is shorter than its header says.
    Truncated,
    /// Wrong magic bytes: not an llmos model.
    BadMagic,
    /// A format version this code does not understand.
    UnsupportedVersion(u32),
    /// An unknown weight-type code.
    UnknownWeightType(u32),
    /// The hyper-parameters are inconsistent (e.g. `dim % n_heads != 0`).
    InvalidConfig(&'static str),
    /// A tensor is not aligned for in-place use.
    Misaligned,
    /// The tokenizer section is malformed.
    BadTokenizer(&'static str),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Truncated => f.write_str("model file is truncated"),
            FormatError::BadMagic => f.write_str("not an llmos model file (bad magic)"),
            FormatError::UnsupportedVersion(v) => write!(f, "unsupported model format version {v}"),
            FormatError::UnknownWeightType(t) => write!(f, "unknown weight type {t}"),
            FormatError::InvalidConfig(why) => write!(f, "invalid model configuration: {why}"),
            FormatError::Misaligned => f.write_str("model data is not 64-byte aligned"),
            FormatError::BadTokenizer(why) => write!(f, "malformed tokenizer: {why}"),
        }
    }
}

/// Decoded fixed-size header.
#[derive(Debug, Clone, Copy)]
pub struct Header {
    /// Hyper-parameters.
    pub config: Config,
    /// Byte offset of the tokenizer section.
    pub tokenizer_offset: usize,
    /// Byte length of the tokenizer section.
    pub tokenizer_size: usize,
    /// Byte offset of the weights section.
    pub weights_offset: usize,
    /// Byte length of the weights section.
    pub weights_size: usize,
    /// Model name (NUL-padded ASCII).
    pub name: [u8; 24],
}

impl Header {
    /// Returns the model name as a string.
    pub fn name(&self) -> &str {
        let end = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..end]).unwrap_or("?")
    }
}

/// Little-endian cursor over a byte slice.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        let end = self.pos.checked_add(n).ok_or(FormatError::Truncated)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(FormatError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.bytes(1)?[0])
    }

    pub(crate) fn u32(&mut self) -> Result<u32, FormatError> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, FormatError> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    pub(crate) fn f32(&mut self) -> Result<f32, FormatError> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub(crate) fn position(&self) -> usize {
        self.pos
    }

    pub(crate) fn align_to(&mut self, align: usize) {
        self.pos = self.pos.div_ceil(align) * align;
    }
}

/// Parses and validates the header at the start of `file`.
///
/// # Errors
///
/// Returns a [`FormatError`] if the file is not a valid model.
pub fn parse_header(file: &[u8]) -> Result<Header, FormatError> {
    let mut r = Reader::new(file);
    if r.bytes(8)? != MAGIC {
        return Err(FormatError::BadMagic);
    }
    let version = r.u32()?;
    if version != FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let header_size = r.u32()? as usize;
    if header_size != HEADER_SIZE {
        return Err(FormatError::InvalidConfig("unexpected header size"));
    }
    let dim = r.u32()? as usize;
    let hidden_dim = r.u32()? as usize;
    let n_layers = r.u32()? as usize;
    let n_heads = r.u32()? as usize;
    let n_kv_heads = r.u32()? as usize;
    let vocab_size = r.u32()? as usize;
    let max_seq_len = r.u32()? as usize;
    let rope_theta = r.f32()?;
    let norm_eps = r.f32()?;
    let weight_type = match r.u32()? {
        0 => WeightType::F32,
        1 => WeightType::Q8,
        other => return Err(FormatError::UnknownWeightType(other)),
    };
    let group_size = r.u32()? as usize;
    let flags = r.u32()?;
    let bos_id = r.u32()?;
    let eos_id = r.u32()?;
    let tokenizer_offset = r.u64()? as usize;
    let tokenizer_size = r.u64()? as usize;
    let weights_offset = r.u64()? as usize;
    let weights_size = r.u64()? as usize;
    let name: [u8; 24] = r.bytes(24)?.try_into().unwrap();

    let config = Config {
        dim,
        hidden_dim,
        n_layers,
        n_heads,
        n_kv_heads,
        vocab_size,
        max_seq_len,
        rope_theta,
        norm_eps,
        weight_type,
        group_size,
        tied_embeddings: flags & 1 != 0,
        bos_id,
        eos_id,
    };
    validate(&config)?;

    let end = |off: usize, len: usize| off.checked_add(len).filter(|&e| e <= file.len());
    if end(tokenizer_offset, tokenizer_size).is_none()
        || end(weights_offset, weights_size).is_none()
    {
        return Err(FormatError::Truncated);
    }
    Ok(Header {
        config,
        tokenizer_offset,
        tokenizer_size,
        weights_offset,
        weights_size,
        name,
    })
}

fn validate(c: &Config) -> Result<(), FormatError> {
    let bad = |why| Err(FormatError::InvalidConfig(why));
    if c.dim == 0 || c.n_heads == 0 || c.n_kv_heads == 0 || c.n_layers == 0 || c.vocab_size == 0 {
        return bad("zero-sized dimension");
    }
    if c.dim % c.n_heads != 0 {
        return bad("dim is not a multiple of n_heads");
    }
    if c.n_heads % c.n_kv_heads != 0 {
        return bad("n_heads is not a multiple of n_kv_heads");
    }
    if c.head_dim() % 2 != 0 {
        return bad("head dimension must be even for rotary embeddings");
    }
    if c.weight_type == WeightType::Q8
        && (c.group_size == 0 || c.dim % c.group_size != 0 || c.hidden_dim % c.group_size != 0)
    {
        return bad("dim and hidden_dim must be multiples of group_size");
    }
    if c.bos_id as usize >= c.vocab_size || c.eos_id as usize >= c.vocab_size {
        return bad("special token id out of range");
    }
    Ok(())
}
