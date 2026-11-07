# The `.model` file format (version 1)

A single little-endian file holding everything needed to run a model: configuration,
tokenizer, weights and, optionally, a pre-computed prompt cache. It is written by
`tools/export_model.py` (and `llm-bake` for the cache) and read by `llm/src/format.rs`,
`llm/src/tokenizer.rs`, `llm/src/model.rs` and `llm/src/prompt_cache.rs`.

The format is designed for **in-place use**. Every tensor starts at a 64-byte aligned file
offset, so a page-aligned RAM disk can be used without copying.

## Layout

```text
offset 0      header (128 bytes)
offset 128    tokenizer section
(aligned 64)  weights section
(aligned 64)  optional prompt-cache section
```

## Header

| Offset | Type | Field |
|-------:|------|-------|
| 0 | `u8[8]` | magic `LLMOSMDL` |
| 8 | `u32` | format version = 1 |
| 12 | `u32` | header size = 128 |
| 16 | `u32` | `dim`: embedding width |
| 20 | `u32` | `hidden_dim`: feed-forward width |
| 24 | `u32` | `n_layers` |
| 28 | `u32` | `n_heads` |
| 32 | `u32` | `n_kv_heads` |
| 36 | `u32` | `vocab_size` |
| 40 | `u32` | `max_seq_len` |
| 44 | `f32` | `rope_theta` |
| 48 | `f32` | `norm_eps` (RMSNorm epsilon) |
| 52 | `u32` | weight type: 0 = f32, 1 = Q8 |
| 56 | `u32` | Q8 group size (0 for f32) |
| 60 | `u32` | flags: bit 0 = tied embeddings |
| 64 | `u32` | BOS token id |
| 68 | `u32` | end-of-turn token id |
| 72 | `u64` | tokenizer section offset |
| 80 | `u64` | tokenizer section size |
| 88 | `u64` | weights section offset |
| 96 | `u64` | weights section size |
| 104 | `u8[24]` | model name, ASCII, NUL padded |

## Tokenizer section

```text
u32 n_tokens
u32 n_merges
n_tokens × { u8 flags; u32 len; u8 bytes[len] }   // flags bit 0: special token
n_merges × { u32 left; u32 right; u32 result }    // in priority order (rank 0 first)
```

Token bytes are **raw bytes**: the exporter undoes GPT-2's byte-to-unicode mapping. Special
tokens such as `<|im_start|>` are matched verbatim in the input before BPE runs.

## Weights section

Tensors appear in this order, each starting at a 64-byte aligned offset. Matrices are
row-major `[rows, cols]` and are applied as `y = W x`.

```text
token_embedding                    [vocab_size, dim]     matrix
for each layer:
    attn_norm                      [dim]                 f32 vector
    wq                             [dim, dim]            matrix
    wk                             [kv_dim, dim]         matrix
    wv                             [kv_dim, dim]         matrix
    wo                             [dim, dim]            matrix
    ffn_norm                       [dim]                 f32 vector
    w1 (gate)                      [hidden_dim, dim]     matrix
    w2 (down)                      [dim, hidden_dim]     matrix
    w3 (up)                        [hidden_dim, dim]     matrix
final_norm                         [dim]                 f32 vector
classifier (only if not tied)      [vocab_size, dim]     matrix
```

`kv_dim = n_kv_heads × dim / n_heads`. Q/K weights keep the Hugging Face layout, which pairs
rotary dimensions `i` and `i + head_dim/2`.

A **matrix** is either:

* **f32:** `rows × cols` floats, or
* **Q8:** `rows × cols` `i8` values, then (at the next aligned offset) `rows × cols / group`
  `f32` scales. Value `k` dequantises to `q[k] × scale[k / group]`.

## Prompt-cache section (optional)

See `llm/src/prompt_cache.rs`. It starts at the first 64-byte boundary after the weights:

```text
u8[8]  magic "LLMOSKVC"
u32    version = 1
u32    n_tokens
u32    n_layers      (must match the header)
u32    kv_dim        (must match the header)
u64    fingerprint   (FNV-1a 64 of the 128 header bytes)
u32    tokens[n_tokens]
-- pad to 64 --
f32    keys  [n_layers][n_tokens][kv_dim]
f32    values[n_layers][n_tokens][kv_dim]
```

A reader ignores the section if it is absent, if any field does not match, or if its tokens
are not a prefix of the prompt being processed.
