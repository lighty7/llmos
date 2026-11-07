#!/usr/bin/env python3
"""Generate reference outputs with Hugging Face `transformers` for llm's tests.

For each test prompt this records the official tokenizer's token ids, the
top-5 next-token logits and a short greedy continuation. The Rust test
`llm/tests/reference.rs` checks that the llmos engine reproduces them.

Requirements (host only, not needed to build llmos):
    pip install torch transformers tokenizers --extra-index-url https://download.pytorch.org/whl/cpu

Usage:
    python3 tools/make_test_vectors.py --src models/src --out models/test-vectors.json
"""

import argparse
import json
from pathlib import Path

import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

PROMPTS = [
    "Once upon a time, there was a",
    "The capital of France is",
    "It's 2024 and   whitespace\n\nmatters: {\"a\": 1}",
    "<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n<|im_start|>user\nWhat is 2+2?<|im_end|>\n<|im_start|>assistant\n",
]
GENERATE_TOKENS = 12


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--src", type=Path, default=Path("models/src"))
    ap.add_argument("--out", type=Path, default=Path("models/test-vectors.json"))
    args = ap.parse_args()

    tok = AutoTokenizer.from_pretrained(args.src)
    model = AutoModelForCausalLM.from_pretrained(args.src, torch_dtype=torch.float32)
    model.eval()

    cases = []
    for prompt in PROMPTS:
        ids = tok(prompt, add_special_tokens=False)["input_ids"]
        with torch.no_grad():
            logits = model(torch.tensor([ids])).logits[0, -1]
            top = torch.topk(logits, 5)
            gen = model.generate(
                torch.tensor([ids]),
                max_new_tokens=GENERATE_TOKENS,
                do_sample=False,
                attention_mask=torch.ones(1, len(ids), dtype=torch.long),
                pad_token_id=tok.eos_token_id,
            )[0, len(ids):].tolist()
        cases.append({
            "prompt": prompt,
            "token_ids": ids,
            "top_ids": top.indices.tolist(),
            "top_logits": top.values.tolist(),
            "greedy": gen,
        })
        print(f"{prompt[:40]!r}: {len(ids)} tokens -> {tok.decode(gen)!r}")
    args.out.write_text(json.dumps({"model": str(args.src), "cases": cases}, indent=1))
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
