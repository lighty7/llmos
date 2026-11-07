//! Measures how well the shell-agent prompt maps requests to actions.
//!
//! ```text
//! cargo run --release -p llm --example agent_eval [-- path/to/model]
//! ```
//!
//! Runs every request in [`CASES`] through the same prompt and constrained
//! decoder the kernel uses, prints each decision, the accuracy and the
//! inference speed. The requests deliberately differ from the prompt's
//! own examples.

use std::time::Instant;

use llm::agent::{SHELL_ACTIONS, prompt_prefix, prompt_request};
use llm::constrained::ActionDecoder;
use llm::{AlignedBuffer, Logits, Model};

/// `(request, expected action)`.
const CASES: &[(&str, &str)] = &[
    (
        "create a file named shopping.txt with eggs and bread",
        "write_file",
    ),
    ("write hello world into greeting.txt", "write_file"),
    (
        "save my password ideas in ideas.txt: use a long phrase",
        "write_file",
    ),
    ("show me my files", "list_files"),
    ("ls", "list_files"),
    ("what is inside readme.txt", "read_file"),
    ("print the contents of config.ini", "read_file"),
    ("delete temp.txt", "delete_file"),
    ("get rid of the file draft.md", "delete_file"),
    ("which cpu does this computer have", "show_hardware"),
    ("list the pci devices", "show_hardware"),
    ("how much ram is being used", "show_memory"),
    ("clear the screen please", "clear_screen"),
    ("what commands can I use", "help"),
    ("good morning!", "say"),
    ("tell me a joke", "say"),
    ("turn off the computer", "poweroff"),
    ("restart the machine", "reboot"),
];

const MACHINE: &str = "CPU: QEMU Virtual CPU with 1 core(s). RAM: 1900 MiB. Devices: host bridge, ISA bridge, SATA controller (AHCI), VGA-compatible display controller, Ethernet controller.";

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "models/llmos.model".into());
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    let buf = AlignedBuffer::from_bytes(&bytes);
    let model = Model::from_bytes(buf.as_bytes()).expect("valid model");
    let decoder = ActionDecoder::new(&model, SHELL_ACTIONS);

    let prefix = model.tokenizer.encode(&prompt_prefix(MACHINE));
    let mut state = model.new_state(1024);
    let t0 = Instant::now();
    decoder
        .prefill(&mut state, &prefix, Logits::None, &mut |_| {})
        .unwrap();
    let prefill = t0.elapsed();
    println!(
        "prompt prefix: {} tokens in {:.2?} ({:.1} tok/s)",
        prefix.len(),
        prefill,
        prefix.len() as f64 / prefill.as_secs_f64()
    );

    let mut correct = 0;
    let mut total_tokens = 0;
    let t0 = Instant::now();
    for (request, expected) in CASES {
        state.rewind(prefix.len());
        let tokens = model.tokenizer.encode(&prompt_request(request));
        let json = decoder.decode(&mut state, &tokens, &mut |_| {}).unwrap();
        total_tokens += state.pos - prefix.len();
        let name = json[12..].split('"').next().unwrap_or("");
        let ok = name == *expected;
        correct += ok as usize;
        println!(
            "{} {request:<55} -> {json}",
            if ok { "ok  " } else { "MISS" }
        );
    }
    let elapsed = t0.elapsed();
    println!(
        "accuracy: {correct}/{} | {:.1} tokens per request, {:.1} tok/s",
        CASES.len(),
        total_tokens as f64 / CASES.len() as f64,
        total_tokens as f64 / elapsed.as_secs_f64()
    );
}
