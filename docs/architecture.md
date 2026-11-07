# llmos architecture

This document describes how llmos is put together and why. It is meant as the starting point
for anyone changing the kernel or the inference engine.

## Design principles

1. **Deterministic code discovers and controls hardware. The LLM only decides.** Hardware
   enumeration reads firmware tables and device registers exactly. The language model reads a
   *summary* of the result and chooses among predefined actions.
2. **Model output is data, never code.** Decoding is constrained to a JSON grammar. The JSON is
   validated, and destructive actions need confirmation.
3. **One inference engine everywhere.** The `llm` crate is `no_std`, so the kernel and the host
   tests run identical code, and correctness can be checked against Hugging Face
   `transformers` on a normal machine.
4. **Small, documented, replaceable parts.** Most drivers are short and written from first
   principles. Crates are used where correctness is subtle (`x86_64` page tables, `bootloader`).

## Boot flow

```text
UEFI firmware (OVMF in QEMU)
  └─ bootloader (UEFI app from the `bootloader` crate)
       • loads the kernel ELF and the RAM disk (the model file) from the FAT EFI partition
       • sets up 4-level paging and maps all physical memory at a dynamic offset
       • collects the memory map, framebuffer, RSDP address
  └─ kernel_main(&BootInfo)                       kernel/src/main.rs
       1. serial + framebuffer console
       2. SSE on (CR0/CR4), GDT + TSS, IDT
       3. OffsetPageTable, frame allocator, heap (half of free RAM, 16–512 MiB)
       4. PIC remap, PIT at 100 Hz, enable interrupts
       5. Inventory: CPUID, ACPI, PCI
       6. Agent: parse the model in place from the RAM disk
       7. Shell loop
```

The image is built on the host by the runner (`src/main.rs`) with
`bootloader::DiskImageBuilder`: a GPT disk with a FAT EFI system partition holding
`efi/boot/bootx64.efi`, the kernel and the `ramdisk`.

## Kernel modules

| Module | Responsibility |
|--------|----------------|
| `serial` | 16550 UART on COM1: polled output and input. Used for logs, the remote shell and tests. |
| `framebuffer`, `console` | Pixel text console. The `print!`/`println!` macros write to serial and screen. |
| `cpu` | Enable SSE/FXSR. |
| `gdt` | GDT with kernel segments and a TSS whose IST\[0\] is the double-fault stack. |
| `interrupts` | IDT: exception handlers (panic with diagnostics), breakpoint, timer and keyboard IRQs. |
| `pic`, `time` | 8259 remapping and EOI. PIT tick counter, uptime and sleeping. |
| `keyboard` | Scancode set 1 decoding into a lock-free SPSC ring buffer, filled from IRQ 1. |
| `memory` | `OffsetPageTable`, `phys_to_virt`, frame allocator over `Usable` regions. |
| `allocator` | Maps the heap range and installs `linked_list_allocator` as the global allocator. |
| `inventory` | `CpuInfo` (CPUID), `acpi` (RSDP→XSDT→MADT), `pci` (configuration ports 0xCF8/0xCFC). |
| `fs` | Flat in-memory file system (`BTreeMap<String, Vec<u8>>`). |
| `actions` | The `Action` enum: JSON parsing, validation, confirmation policy, execution. |
| `agent` | Owns the model and inference state. Builds prompts, caches the prefix, decodes actions. |
| `shell` | Line editor (keyboard + serial) and command dispatch. |
| `selftest` | Checks run on the live system. |

### Interrupt safety

* The keyboard queue is lock-free, so the IRQ handler can never deadlock against the code it
  interrupted.
* Console output disables interrupts while it holds the serial and screen locks.
* Nothing allocates in interrupt context.

## The inference engine (`llm`)

| Module | Responsibility |
|--------|----------------|
| `format` | Header parsing and validation of the `.model` file ([model-format.md](model-format.md)). |
| `tokenizer` | Byte-level BPE: special tokens, digit splitting, GPT-2 pre-tokenisation regex, merges. |
| `model` | Forward pass: RMSNorm, RoPE ("rotate half"), grouped-query attention with a KV cache, SwiGLU. f32 or Q8 matmuls. |
| `math` | Vectorisable dot products, softmax, norms, `libm` wrappers. |
| `constrained` | Grammar-constrained decoding of `{"action": ..., fields...}`. |
| `agent` | The action list and prompt shared by the kernel and the evaluation harness. |
| `prompt_cache` | Baking and loading the KV cache of the machine-independent prompt. |
| `generate` | Plain greedy generation, used for diagnostics and tests. |

Weights are used **in place**. The file is laid out with 64-byte aligned tensors, so the kernel
borrows `&[i8]`/`&[f32]` slices straight from the RAM disk. Only the activations and the KV
cache are allocated: about 45 MiB for a 1024-token context with SmolLM2-135M.

### Constrained decoding

At each step the decoder computes logits only for the tokens the grammar allows
(`Logits::Subset`):

1. **Action name:** a prefix trie over the token sequences of the allowed names.
2. **Field values:** tokens whose bytes fit the field's character set (file names:
   `[A-Za-z0-9._-]`; text: printable ASCII without `"` or `\`), plus "closing quote" tokens.
   Values have a token limit and stop when they start repeating.
3. **Structure:** braces, quotes and keys are fed as fixed text.

The result is always syntactically valid JSON naming a known action. `Action::from_json` then
checks the semantics.

### Prompt caching

The prompt is `static_prompt()` (instructions and examples, identical on every machine),
followed by a hardware note and the user turn. `llm-bake` runs the static part on the host and
appends its KV cache to the model file. At boot the kernel copies it into the state, processes
only the remaining ~50 tokens once, and rewinds to that point for every request.

### Verification

`llm/tests/reference.rs` compares against `transformers`, using vectors from
`tools/make_test_vectors.py`:

* tokenizer output must be identical,
* f32 logits must match within 1e-2, and greedy continuations exactly,
* Q8 must agree on the first generated token,
* the baked cache must reproduce a fresh computation within 1e-3.

## Performance notes

| Setup | Speed (SmolLM2-135M, Q8, one core) |
|-------|-------------------------------------|
| Host, native | ~19 tokens/s |
| QEMU with KVM | close to native |
| QEMU without KVM (software CPU) | ~0.7 tokens/s |

The classifier (vocabulary × dim) is the largest single matrix. Constrained decoding avoids
most of it by scoring only the allowed tokens.

## Known limitations

* Single core; no SMP or scheduler yet.
* Files live in RAM and are lost at power-off (Phase 6 adds a disk).
* Power-off uses QEMU/Bochs ports. Real ACPI S5 needs an AML interpreter.
* The 135M model maps about 70% of unseen paraphrased requests to the right action
  (`agent_eval`). Built-in commands remain available and exact.
