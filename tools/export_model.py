#!/usr/bin/env python3
"""Convert a Hugging Face Llama-architecture checkpoint to an llmos `.model` file.

The output is the single-file format read by `llm/src/format.rs` and
documented in `docs/model-format.md`: a 128-byte header, the byte-level BPE
tokenizer, then every weight tensor at a 64-byte aligned offset.

Requirements: Python 3.9+ and numpy. No PyTorch needed: `.safetensors` files
are parsed directly.

Examples:
    # Download SmolLM2-135M-Instruct and write models/llmos.model (int8, ~150 MB)
    python3 tools/export_model.py --download HuggingFaceTB/SmolLM2-135M-Instruct

    # Convert a local checkpoint directory, keeping f32 weights
    python3 tools/export_model.py --src models/src --dtype f32 --out models/smollm2-f32.model
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
import urllib.request
from pathlib import Path

import numpy as np

MAGIC = b"LLMOSMDL"
FORMAT_VERSION = 1
HEADER_SIZE = 128
ALIGN = 64
WEIGHT_TYPES = {"f32": 0, "q8": 1}
DOWNLOAD_FILES = ["config.json", "tokenizer.json", "model.safetensors"]


# --------------------------------------------------------------------------
# Input: safetensors + tokenizer.json
# --------------------------------------------------------------------------

def load_safetensors(path: Path) -> dict[str, np.ndarray]:
    """Read every tensor of a .safetensors file as float32 numpy arrays."""
    data = path.read_bytes()
    (header_len,) = struct.unpack("<Q", data[:8])
    header = json.loads(data[8 : 8 + header_len])
    base = 8 + header_len
    tensors = {}
    for name, info in header.items():
        if name == "__metadata__":
            continue
        start, end = info["data_offsets"]
        raw = data[base + start : base + end]
        dtype = info["dtype"]
        if dtype == "F32":
            arr = np.frombuffer(raw, dtype="<f4")
        elif dtype == "F16":
            arr = np.frombuffer(raw, dtype="<f2").astype(np.float32)
        elif dtype == "BF16":
            # bfloat16 is the upper half of a float32.
            arr = (np.frombuffer(raw, dtype="<u2").astype(np.uint32) << 16).view(np.float32)
        else:
            raise ValueError(f"unsupported tensor dtype {dtype} for {name}")
        tensors[name] = arr.reshape(info["shape"])
    return tensors


def bytes_to_unicode() -> dict[int, str]:
    """GPT-2's reversible byte -> printable-unicode mapping."""
    bs = list(range(ord("!"), ord("~") + 1)) + list(range(ord("¡"), ord("¬") + 1)) + list(range(ord("®"), ord("ÿ") + 1))
    cs = bs[:]
    n = 0
    for b in range(256):
        if b not in bs:
            bs.append(b)
            cs.append(256 + n)
            n += 1
    return dict(zip(bs, map(chr, cs)))


def load_tokenizer(path: Path, vocab_size: int):
    """Return (tokens, merges): tokens = [(flags, raw_bytes)], merges = [(l, r, result)]."""
    tj = json.loads(path.read_text(encoding="utf-8"))
    model = tj["model"]
    if model["type"] != "BPE":
        raise ValueError("only BPE tokenizers are supported")
    byte_decoder = {c: b for b, c in bytes_to_unicode().items()}
    specials = {t["id"]: t["content"] for t in tj.get("added_tokens", []) if t.get("special")}

    tokens: list[tuple[int, bytes] | None] = [None] * vocab_size
    for text, idx in model["vocab"].items():
        if idx in specials:
            tokens[idx] = (1, specials[idx].encode("utf-8"))
        else:
            tokens[idx] = (0, bytes(byte_decoder[c] for c in text))
    for idx, text in specials.items():
        tokens[idx] = (1, text.encode("utf-8"))
    for i, t in enumerate(tokens):
        if t is None:  # Padding entries beyond the tokenizer's vocabulary.
            tokens[i] = (1, f"<|unused{i}|>".encode())

    vocab = model["vocab"]
    merges = []
    for m in model["merges"]:
        left, right = m.split(" ", 1) if isinstance(m, str) else m
        merges.append((vocab[left], vocab[right], vocab[left + right]))
    return tokens, merges


# --------------------------------------------------------------------------
# Output
# --------------------------------------------------------------------------

def pad_to(buf: bytearray, align: int) -> None:
    buf.extend(b"\0" * (-len(buf) % align))


def quantize_q8(w: np.ndarray, group: int) -> tuple[np.ndarray, np.ndarray]:
    """Symmetric int8 quantisation in groups along the last axis."""
    flat = w.astype(np.float32).reshape(-1, group)
    scale = np.abs(flat).max(axis=1) / 127.0
    inv = np.where(scale > 0, 1.0 / np.where(scale > 0, scale, 1), 0)
    q = np.clip(np.round(flat * inv[:, None]), -127, 127).astype(np.int8)
    return q.reshape(-1), scale.astype("<f4")


def write_tensor(buf: bytearray, w: np.ndarray, dtype: str, group: int) -> float:
    """Append a matrix in the chosen dtype; returns the max abs error."""
    pad_to(buf, ALIGN)
    if dtype == "f32":
        buf.extend(w.astype("<f4").tobytes())
        return 0.0
    q, scale = quantize_q8(w, group)
    buf.extend(q.tobytes())
    pad_to(buf, ALIGN)
    buf.extend(scale.tobytes())
    recon = (q.reshape(-1, group).astype(np.float32) * scale[:, None]).reshape(w.shape)
    return float(np.abs(recon - w).max())


def write_vector(buf: bytearray, v: np.ndarray) -> None:
    pad_to(buf, ALIGN)
    buf.extend(v.astype("<f4").tobytes())


def export(src: Path, out: Path, dtype: str, group: int, name: str | None, seq_len: int | None) -> None:
    cfg = json.loads((src / "config.json").read_text())
    if cfg.get("model_type") not in ("llama", "mistral", "qwen2") or cfg.get("attention_bias"):
        print(f"warning: model_type={cfg.get('model_type')} may not be supported", file=sys.stderr)
    dim = cfg["hidden_size"]
    hidden = cfg["intermediate_size"]
    n_layers = cfg["num_hidden_layers"]
    n_heads = cfg["num_attention_heads"]
    n_kv = cfg.get("num_key_value_heads", n_heads)
    vocab = cfg["vocab_size"]
    max_seq = seq_len or cfg.get("max_position_embeddings", 2048)
    theta = float(cfg.get("rope_theta", 10000.0))
    eps = float(cfg.get("rms_norm_eps", 1e-5))
    tied = bool(cfg.get("tie_word_embeddings", False))
    if cfg.get("rope_scaling"):
        raise ValueError("rope_scaling is not supported yet")
    if dtype == "q8" and (dim % group or hidden % group):
        raise ValueError(f"dim ({dim}) and hidden ({hidden}) must be multiples of --group-size {group}")

    print(f"loading weights from {src}")
    tensors = {}
    for f in sorted(src.glob("*.safetensors")):
        tensors.update(load_safetensors(f))
    tokens, merges = load_tokenizer(src / "tokenizer.json", vocab)

    tok_name = json.loads((src / "tokenizer.json").read_text()).get("added_tokens", [])
    special_ids = {t["content"]: t["id"] for t in tok_name}
    bos = cfg.get("bos_token_id", 1) or 0
    eos = special_ids.get("<|im_end|>", cfg.get("eos_token_id", 2))
    if isinstance(eos, list):
        eos = eos[0]

    # Tokenizer section.
    tok = bytearray(struct.pack("<II", len(tokens), len(merges)))
    for flags, raw in tokens:
        tok.extend(struct.pack("<BI", flags, len(raw)))
        tok.extend(raw)
    for m in merges:
        tok.extend(struct.pack("<III", *m))

    # Weights, in the order `llm/src/model.rs` reads them.
    body = bytearray()
    tok_offset = HEADER_SIZE
    body.extend(tok)
    pad_to(body, ALIGN)
    weights_offset = HEADER_SIZE + len(body)
    w = bytearray()
    # Keep absolute alignment: weights start at an aligned file offset.
    assert weights_offset % ALIGN == 0

    def t(key: str) -> np.ndarray:
        return tensors[key]

    max_err = 0.0
    max_err = max(max_err, write_tensor(w, t("model.embed_tokens.weight"), dtype, group))
    for i in range(n_layers):
        p = f"model.layers.{i}."
        write_vector(w, t(p + "input_layernorm.weight"))
        for proj in ("q_proj", "k_proj", "v_proj", "o_proj"):
            max_err = max(max_err, write_tensor(w, t(p + f"self_attn.{proj}.weight"), dtype, group))
        write_vector(w, t(p + "post_attention_layernorm.weight"))
        for proj in ("gate_proj", "down_proj", "up_proj"):
            max_err = max(max_err, write_tensor(w, t(p + f"mlp.{proj}.weight"), dtype, group))
        print(f"\r  layer {i + 1}/{n_layers}", end="", flush=True)
    print()
    write_vector(w, t("model.norm.weight"))
    if not tied:
        max_err = max(max_err, write_tensor(w, t("lm_head.weight"), dtype, group))

    model_name = (name or cfg.get("_name_or_path") or src.name)[:24].encode("ascii", "replace")
    header = struct.pack(
        "<8sII" + "I" * 7 + "ff" + "I" * 5 + "QQQQ" + "24s",
        MAGIC, FORMAT_VERSION, HEADER_SIZE,
        dim, hidden, n_layers, n_heads, n_kv, vocab, max_seq,
        theta, eps,
        WEIGHT_TYPES[dtype], group if dtype == "q8" else 0, 1 if tied else 0, bos, eos,
        tok_offset, len(tok), weights_offset, len(w),
        model_name,
    )
    assert len(header) == HEADER_SIZE, len(header)

    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("wb") as f:
        f.write(header)
        f.write(body)
        f.write(w)
    params = sum(v.size for v in tensors.values())
    print(f"wrote {out} ({out.stat().st_size / 2**20:.1f} MiB, {params / 1e6:.1f}M parameters, {dtype})")
    if dtype == "q8":
        print(f"max quantisation error: {max_err:.5f}")


def download(repo: str, dest: Path) -> None:
    dest.mkdir(parents=True, exist_ok=True)
    for fname in DOWNLOAD_FILES:
        target = dest / fname
        if target.exists():
            continue
        url = f"https://huggingface.co/{repo}/resolve/main/{fname}"
        print(f"downloading {url}")
        urllib.request.urlretrieve(url, target)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--src", type=Path, default=Path("models/src"), help="checkpoint directory (config.json, tokenizer.json, *.safetensors)")
    ap.add_argument("--download", metavar="REPO", help="download REPO from Hugging Face into --src first")
    ap.add_argument("--out", type=Path, default=Path("models/llmos.model"), help="output file")
    ap.add_argument("--dtype", choices=WEIGHT_TYPES, default="q8", help="weight storage (default: q8)")
    ap.add_argument("--group-size", type=int, default=64, help="Q8 group size (default: 64)")
    ap.add_argument("--name", help="model name stored in the header (max 24 chars)")
    ap.add_argument("--seq-len", type=int, help="override the maximum context length")
    args = ap.parse_args()
    if args.download:
        download(args.download, args.src)
    export(args.src, args.out, args.dtype, args.group_size, args.name, args.seq_len)


if __name__ == "__main__":
    main()
