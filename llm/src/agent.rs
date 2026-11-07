//! The llmos shell-agent protocol: which actions the model may choose and
//! the prompt that teaches it how.
//!
//! Shared by the kernel (which executes the actions) and the host-side
//! evaluation harness (`cargo run --release -p llm --example agent_eval`),
//! so prompt changes can be measured on a fast machine before they ship.
//!
//! The prompt uses the ChatML template of SmolLM2 / Qwen models:
//!
//! ```text
//! <|im_start|>system\n{instructions + examples + machine}<|im_end|>\n
//! <|im_start|>user\n{request}<|im_end|>\n
//! <|im_start|>assistant\n{"action": "
//! ```
//!
//! Everything up to and including `<|im_start|>user\n` is constant for a
//! boot, so the kernel processes it once and reuses its key/value cache. The
//! part before the hardware description ([`static_prompt`]) is even the same
//! on every machine, so its cache can be baked into the model file.

use alloc::format;
use alloc::string::String;

use crate::constrained::{ActionSpec, FieldKind, FieldSpec, JSON_PREFIX};

const PATH: FieldSpec = FieldSpec {
    name: "path",
    kind: FieldKind::Path,
};
const CONTENT: FieldSpec = FieldSpec {
    name: "content",
    kind: FieldKind::Text,
};
const TEXT: FieldSpec = FieldSpec {
    name: "text",
    kind: FieldKind::Text,
};

/// Every action the shell agent can request, with its JSON fields.
pub static SHELL_ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        name: "list_files",
        fields: &[],
    },
    ActionSpec {
        name: "read_file",
        fields: &[PATH],
    },
    ActionSpec {
        name: "write_file",
        fields: &[PATH, CONTENT],
    },
    ActionSpec {
        name: "delete_file",
        fields: &[PATH],
    },
    ActionSpec {
        name: "show_hardware",
        fields: &[],
    },
    ActionSpec {
        name: "show_memory",
        fields: &[],
    },
    ActionSpec {
        name: "clear_screen",
        fields: &[],
    },
    ActionSpec {
        name: "help",
        fields: &[],
    },
    ActionSpec {
        name: "say",
        fields: &[TEXT],
    },
    ActionSpec {
        name: "poweroff",
        fields: &[],
    },
    ActionSpec {
        name: "reboot",
        fields: &[],
    },
];

/// Instructions, constant across machines (so their key/value cache can be
/// baked into the model file ahead of time, see [`static_prompt`]).
const INSTRUCTIONS: &str =
    "You are llmos, the shell of a computer. Answer every request with exactly one JSON action.
Actions:
list_files: list the files
read_file(path): show what is inside a file
write_file(path, content): create or change a file
delete_file(path): remove a file
show_hardware: show the CPU, devices and hardware
show_memory: show memory and RAM usage
clear_screen: clear the screen
help: list the commands
say(text): reply in words to a question or greeting
poweroff: turn the computer off
reboot: restart the computer";

/// Worked examples, given to the model as earlier conversation turns
/// (small instruct models imitate turns far better than inline examples).
/// Every action has at least one example.
const EXAMPLES: &[(&str, &str)] = &[
    (
        "make a file called todo.txt that says buy milk",
        r#"{"action": "write_file", "path": "todo.txt", "content": "buy milk"}"#,
    ),
    ("what files do I have", r#"{"action": "list_files"}"#),
    (
        "show me what is in notes.txt",
        r#"{"action": "read_file", "path": "notes.txt"}"#,
    ),
    (
        "remove the file old.log",
        r#"{"action": "delete_file", "path": "old.log"}"#,
    ),
    (
        "what processor and devices does this machine have",
        r#"{"action": "show_hardware"}"#,
    ),
    ("how much memory is free", r#"{"action": "show_memory"}"#),
    (
        "hi, who are you?",
        r#"{"action": "say", "text": "I am llmos, the shell of this computer."}"#,
    ),
    ("clear the terminal", r#"{"action": "clear_screen"}"#),
    ("which commands are there", r#"{"action": "help"}"#),
    ("shut the computer down", r#"{"action": "poweroff"}"#),
    ("please reboot", r#"{"action": "reboot"}"#),
];

/// The machine-independent start of every prompt: system instructions and
/// example turns. Its key/value cache can be computed on a fast host and
/// stored in the model file (`llm-bake`), saving minutes at boot on slow
/// or emulated CPUs.
pub fn static_prompt() -> String {
    let mut p = format!("<|im_start|>system\n{INSTRUCTIONS}<|im_end|>\n");
    for (user, assistant) in EXAMPLES {
        p.push_str(&format!(
            "<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n{assistant}<|im_end|>\n"
        ));
    }
    p
}

/// Builds the constant prompt prefix for this machine, ending just before
/// the user's text: [`static_prompt`], then a short system note describing
/// the hardware, then the opening of the user turn.
pub fn prompt_prefix(machine_summary: &str) -> String {
    let mut p = static_prompt();
    p.push_str(&format!(
        "<|im_start|>system\nThis computer: {machine_summary}<|im_end|>\n<|im_start|>user\n"
    ));
    p
}

/// Builds the per-request part of the prompt, which follows
/// [`prompt_prefix`] and ends with the opening of the JSON answer.
pub fn prompt_request(request: &str) -> String {
    format!("{request}<|im_end|>\n<|im_start|>assistant\n{JSON_PREFIX}")
}
