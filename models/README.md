# Models

Model files are large binaries and are **not committed** (this directory is git-ignored
except for this file). The runner packs `models/llmos.model` into the boot RAM disk when it
exists.

## Recommended: SmolLM2-135M-Instruct

```bash
python3 tools/export_model.py --download HuggingFaceTB/SmolLM2-135M-Instruct
cargo run --release -p llm --bin llm-bake
```

* `export_model.py` downloads `config.json`, `tokenizer.json` and `model.safetensors` into
  `models/src/` and writes `models/llmos.model`: int8 (Q8) weights, about 140 MB. Only numpy
  is needed.
* `llm-bake` appends the pre-computed key/value cache of the fixed prompt (about 20 MB). Without
  it, the first request on an emulated CPU spends several minutes reading the system prompt.
  Re-run it after editing `llm/src/agent.rs`. A stale cache is detected and ignored, but then
  it gives no speed-up.

Memory: the guest needs about 140 MB for the RAM disk plus about 50 MB of inference state. The
runner gives QEMU 2 GiB by default.

## Other models

Any Hugging Face checkpoint with the Llama architecture and a byte-level BPE tokenizer should
convert: SmolLM2-360M/1.7B, TinyLlama, Llama-3.2, Qwen2 without biases. Larger models are
smarter but slower, and the prompt in `llm/src/agent.rs` assumes the ChatML template.

```bash
python3 tools/export_model.py --download HuggingFaceTB/SmolLM2-360M-Instruct \
    --src models/smollm2-360m --out models/llmos.model
```

## Files used by the tests

```bash
python3 tools/export_model.py --dtype f32 --out models/smollm2-f32.model
pip install torch transformers --extra-index-url https://download.pytorch.org/whl/cpu
python3 tools/make_test_vectors.py
cargo test --release -p llm
```
