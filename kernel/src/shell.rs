//! The interactive shell.
//!
//! Reads lines from the PS/2 keyboard *and* the serial port (so the system can
//! be driven from a host terminal or an automated test), then:
//!
//! 1. runs built-in commands (`ls`, `cat`, `hw`, `selftest`, ...) directly, or
//! 2. hands anything else to the LLM agent, which turns natural language into
//!    a validated [`Action`] (see [`crate::agent`]).

use alloc::string::String;

use crate::actions;
use crate::framebuffer::Color;
use crate::inventory::Inventory;
use crate::{agent, console, keyboard, print, println, selftest, serial, time};

/// Longest accepted input line.
const MAX_LINE: usize = 512;

/// State shared by shell commands.
pub struct Context {
    /// Hardware inventory collected at boot.
    pub inventory: Inventory,
    /// The LLM agent, if a model was loaded.
    pub agent: Option<agent::Agent>,
}

/// Prints the list of built-in commands.
pub fn print_help() {
    console::set_color(Color::YELLOW);
    println!("Built-in commands:");
    console::set_color(Color::WHITE);
    println!("  help                 show this help");
    println!("  ls                   list files");
    println!("  cat <file>           print a file");
    println!("  write <file> <text>  create or overwrite a file");
    println!("  rm <file>            delete a file");
    println!("  disks                list disks and partitions");
    println!("  install [disk]       install llmos onto an empty disk");
    println!("  info                 hardware summary");
    println!("  hw                   full hardware inventory as JSON");
    println!("  mem                  memory usage");
    println!("  uptime               time since boot");
    println!("  model                LLM model details");
    println!("  ask <request>        send a request to the LLM");
    println!("  selftest             run kernel self-tests");
    println!("  clear                clear the screen");
    println!("  reboot / poweroff    restart or stop the machine");
    println!(
        "Anything else is sent to the LLM, e.g. `make a file called todo.txt saying buy milk`."
    );
}

/// Blocks until a byte arrives from the keyboard or serial port.
fn read_byte() -> u8 {
    loop {
        if let Some(b) = keyboard::read_char().or_else(serial::try_read_byte) {
            return b;
        }
        // Sleep until the next interrupt (timer at 100 Hz, or a key press).
        x86_64::instructions::hlt();
    }
}

/// Line editor state that survives between lines.
struct LineReader {
    /// The previous byte was `\r` (so a following `\n` is part of CRLF).
    last_was_cr: bool,
}

impl LineReader {
    /// Reads one line with echo and backspace support.
    fn read_line(&mut self) -> String {
        let mut line = String::new();
        loop {
            let b = read_byte();
            match b {
                b'\n' if self.last_was_cr => {
                    self.last_was_cr = false;
                    continue;
                }
                b'\r' | b'\n' => {
                    self.last_was_cr = b == b'\r';
                    println!();
                    return line;
                }
                0x08 | 0x7F => {
                    self.last_was_cr = false;
                    if line.pop().is_some() {
                        print!("\x08 \x08");
                    }
                }
                0x20..=0x7E if line.len() < MAX_LINE => {
                    self.last_was_cr = false;
                    line.push(b as char);
                    print!("{}", b as char);
                }
                _ => self.last_was_cr = false,
            }
        }
    }
}

/// Asks a yes/no question; returns `true` for `y`/`yes`.
pub fn confirm(question: &str) -> bool {
    console::set_color(Color::YELLOW);
    print!("{question} [y/N] ");
    console::set_color(Color::WHITE);
    let answer = LineReader { last_was_cr: false }.read_line();
    matches!(answer.trim(), "y" | "Y" | "yes" | "YES")
}

/// Runs the shell forever.
pub fn run(mut ctx: Context) -> ! {
    println!();
    println!("Type `help` for commands, or just tell llmos what you want.");
    // Marker used by the automated boot test to know the shell is up.
    println!("shell ready");
    let mut reader = LineReader { last_was_cr: false };
    loop {
        console::set_color(Color::CYAN);
        print!("llmos> ");
        console::set_color(Color::WHITE);
        let line = reader.read_line();
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        handle_line(&mut ctx, line);
    }
}

/// Dispatches one input line.
fn handle_line(ctx: &mut Context, line: &str) {
    let (cmd, rest) = actions::split_first_word(line);
    match cmd {
        "hw" => println!("{}", ctx.inventory.to_json()),
        "uptime" => {
            let ms = time::uptime_ms();
            println!("up {}.{:02}s", ms / 1000, (ms % 1000) / 10);
        }
        "selftest" => selftest::run(ctx),
        "model" => match &ctx.agent {
            Some(agent) => agent.print_info(),
            None => println!("no model loaded (see models/README.md)"),
        },
        "ask" => ask(ctx, rest),
        _ => match actions::from_command(line) {
            // Typed commands run directly, except installing, which always
            // shows its plan and asks first.
            Some(Ok(action @ actions::Action::Install { .. })) => {
                println!("{}", action.describe());
                if confirm("This will write to the disk. Continue?") {
                    action.execute(ctx);
                } else {
                    println!("cancelled");
                }
            }
            Some(Ok(action)) => action.execute(ctx),
            Some(Err(e)) => println!("error: {e}"),
            None => ask(ctx, line),
        },
    }
}

/// Sends a natural-language request to the LLM agent and runs its decision.
fn ask(ctx: &mut Context, request: &str) {
    if request.is_empty() {
        println!("usage: ask <request>");
        return;
    }
    let Some(agent) = ctx.agent.as_mut() else {
        println!(
            "unknown command `{request}`; no LLM model is loaded, so only built-in commands work (type `help`)"
        );
        return;
    };
    let action = match agent.decide(request) {
        Ok(action) => action,
        Err(e) => {
            println!("the model's answer was rejected: {e}");
            return;
        }
    };
    console::set_color(Color::GREEN);
    println!("llm -> {}", action.describe());
    console::set_color(Color::WHITE);
    if action.needs_confirmation() && !confirm("Allow this?") {
        println!("cancelled");
        return;
    }
    action.execute(ctx);
}
