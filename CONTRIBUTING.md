# Contributing to llmos

Thanks for your interest! llmos is a learning-friendly project: small modules, documented
`unsafe`, and tests you can run on your own machine.

## Setup

```bash
git clone https://github.com/lighty7/llmos.git && cd llmos
sudo apt install qemu-system-x86 ovmf python3-numpy   # or your distribution's equivalents
cargo run -- test --no-model                          # build and run the boot test
```

`rust-toolchain.toml` pins the nightly toolchain, and `rustup` installs it on the first build.

## Workflow

1. Create a branch from `dev`: `git checkout -b feature/short-description dev`.
2. Make a focused change: one feature or fix per pull request.
3. Run the checks below.
4. Update the documentation and add a line to `CHANGELOG.md` under *Unreleased*.
5. Open a pull request against `dev`.

## Checks before a pull request

```bash
cargo fmt --all && (cd kernel && cargo fmt)
cargo clippy -p llm -p llmos && (cd kernel && cargo clippy)
cargo test -p llm                      # unit tests (fast)
cargo test --release -p llm            # + reference tests if the model files exist
cargo run -- test --no-model           # boots the kernel in QEMU and checks the shell
```

## Coding standards

* **Rust 2024 edition**, `rustfmt` defaults with a maximum width of 100 (`.editorconfig`).
* **Documentation:** every module starts with a `//!` comment explaining its purpose. Every
  public item has a `///` comment. Use the standard sections where they apply:
  * `# Safety` on every `unsafe fn` (what the caller must guarantee),
  * `# Errors` on functions returning `Result`,
  * `# Panics` on functions that can panic.
* **`unsafe`:** keep blocks small. Put a `// SAFETY:` comment directly above each one,
  explaining why it is sound.
* **No allocation in interrupt handlers.** Prefer lock-free structures for data shared with
  IRQs.
* **The LLM never gets raw power.** New capabilities are new `Action` variants with
  validation and, if destructive, confirmation. Add them to `llm::agent::SHELL_ACTIONS` and
  give them a prompt example. The `selftest` command checks that both lists agree.
* Prompt changes should come with a before/after `agent_eval` accuracy figure.

## Commit messages

Use the imperative mood and a short summary line (≤ 72 characters), then a blank line and the
details: what and why, not how.

```text
Add virtio-blk driver for persistent storage

Legacy (transitional) virtio over PCI I/O ports, one request queue,
polled completion. Enough for QEMU and most hypervisors.
```

## Project layout

See [README.md](README.md#repository-layout) and [docs/architecture.md](docs/architecture.md).
