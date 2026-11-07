//! Serial shell and LLM agent for the AArch64 kernel.
//!
//! Same commands and agent protocol as the x86-64 shell
//! ([`llm::agent::SHELL_ACTIONS`]). Actions that need hardware this port has
//! no drivers for yet (disks, installation) are answered with an explanation
//! instead of being executed.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use llm::agent::{SHELL_ACTIONS, prompt_prefix, prompt_request};
use llm::constrained::{ActionDecoder, Event};
use llm::prompt_cache::PromptCache;
use llm::{FormatError, Logits, Model, State, TokenId};
use serde::Deserialize;

use crate::platform::{self, Info};
use crate::{MIB, fs, print, println, uart};

/// Context window for the agent, in tokens.
const SEQ_LEN: usize = 1024;

/// State shared by the shell commands.
pub struct Context {
    /// Board information.
    pub info: Info,
    /// The LLM agent, if a model was loaded from the initrd.
    pub agent: Option<Agent>,
}

/// The model plus its inference state (see the x86-64 `agent` module, which
/// this mirrors).
pub struct Agent {
    model: &'static Model<'static>,
    decoder: ActionDecoder<'static, 'static>,
    state: State,
    prefix: Vec<TokenId>,
    cache: Option<PromptCache<'static>>,
    prefix_ready: bool,
}

impl Agent {
    /// Loads the model from the initrd contents.
    ///
    /// # Errors
    ///
    /// Fails if the initrd is not a valid model file.
    pub fn load(file: &'static [u8], machine_summary: &str) -> Result<Self, FormatError> {
        let model: &'static Model<'static> = Box::leak(Box::new(Model::from_bytes(file)?));
        Ok(Self {
            decoder: ActionDecoder::new(model, SHELL_ACTIONS),
            state: model.new_state(SEQ_LEN),
            prefix: model.tokenizer.encode(&prompt_prefix(machine_summary)),
            cache: PromptCache::find(file, model),
            prefix_ready: false,
            model,
        })
    }

    /// One-line description.
    pub fn summary(&self) -> String {
        alloc::format!(
            "{} ({:.0}M parameters, {:?}), prompt cache: {}",
            self.model.name,
            self.model.config.parameter_count() as f64 / 1e6,
            self.model.config.weight_type,
            if self.cache.is_some() { "yes" } else { "no" }
        )
    }

    /// Turns a request into action JSON.
    fn decide(&mut self, request: &str) -> Result<String, llm::constrained::DecodeError> {
        if !self.prefix_ready {
            self.state.rewind(0);
            let cached = self
                .cache
                .as_ref()
                .map_or(0, |c| c.apply(&mut self.state, &self.prefix));
            println!(
                "reading the system prompt ({} tokens, {cached} cached)...",
                self.prefix.len()
            );
            let start = platform::uptime_ms();
            self.decoder.prefill(
                &mut self.state,
                &self.prefix[cached..],
                Logits::None,
                &mut |_| {},
            )?;
            println!("  done in {} ms", platform::uptime_ms() - start);
            self.prefix_ready = true;
        }
        self.state.rewind(self.prefix.len());
        let tokens = self.model.tokenizer.encode(&prompt_request(request));
        let json = self.decoder.decode(&mut self.state, &tokens, &mut |e| {
            if let Event::Output(t) = e {
                print!("{t}");
            }
        })?;
        println!();
        Ok(json)
    }
}

/// Raw JSON action, as produced by the constrained decoder.
#[derive(Deserialize)]
struct RawAction {
    action: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

fn read_byte() -> u8 {
    loop {
        if let Some(b) = uart::try_read_byte() {
            return b;
        }
        core::hint::spin_loop();
    }
}

fn read_line() -> String {
    let mut line = String::new();
    loop {
        match read_byte() {
            b'\r' | b'\n' => {
                println!();
                return line;
            }
            0x08 | 0x7F => {
                if line.pop().is_some() {
                    print!("\x08 \x08");
                }
            }
            b @ 0x20..=0x7E if line.len() < 512 => {
                line.push(b as char);
                print!("{}", b as char);
            }
            _ => {}
        }
    }
}

fn confirm(question: &str) -> bool {
    print!("{question} [y/N] ");
    matches!(read_line().trim(), "y" | "Y" | "yes")
}

fn print_help() {
    println!("Commands: help, info, hw, ls, cat <f>, write <f> <text>, rm <f>, mem, uptime,");
    println!("          model, ask <request>, selftest, reboot, poweroff");
    println!("Anything else is sent to the LLM.");
}

/// Runs the shell forever.
pub fn run(mut ctx: Context) -> ! {
    println!("\nType `help` for commands, or just tell llmos what you want.");
    println!("shell ready");
    loop {
        print!("llmos> ");
        let line = read_line();
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (cmd, rest) = line
            .split_once(' ')
            .map_or((line, ""), |(c, r)| (c, r.trim()));
        match cmd {
            "help" => print_help(),
            "info" => ctx.info.print(),
            "hw" => println!(
                "{}",
                serde_json::to_string_pretty(&ctx.info).unwrap_or_default()
            ),
            "ls" => list_files(),
            "cat" => read_file(rest),
            "write" => {
                let (name, text) = rest.split_once(' ').unwrap_or((rest, ""));
                write_file(name, text);
            }
            "rm" => delete_file(rest),
            "mem" => show_memory(),
            "uptime" => println!("up {} ms", platform::uptime_ms()),
            "model" => match &ctx.agent {
                Some(a) => println!("{}", a.summary()),
                None => println!("no model loaded (pass one as the initrd)"),
            },
            "selftest" => selftest(&ctx),
            "reboot" => platform::reboot(ctx.info.psci.as_deref()),
            "poweroff" | "shutdown" | "exit" => {
                println!("powering off");
                platform::poweroff(ctx.info.psci.as_deref())
            }
            "ask" => ask(&mut ctx, rest),
            _ => ask(&mut ctx, line),
        }
    }
}

fn list_files() {
    let files = fs::list();
    if files.is_empty() {
        println!("(no files)");
    }
    for (name, size) in files {
        println!("  {name:<32} {size:>8} bytes");
    }
}

fn read_file(name: &str) {
    match fs::read(name) {
        Ok(data) => println!("{}", String::from_utf8_lossy(&data)),
        Err(e) => println!("error: {e}"),
    }
}

fn write_file(name: &str, text: &str) {
    match fs::write(name, text.as_bytes()) {
        Ok(()) => println!("wrote {} bytes to {name}", text.len()),
        Err(e) => println!("error: {e}"),
    }
}

fn delete_file(name: &str) {
    match fs::remove(name) {
        Ok(()) => println!("removed {name}"),
        Err(e) => println!("error: {e}"),
    }
}

fn show_memory() {
    let (used, free) = crate::heap_usage();
    println!(
        "heap: {} KiB used, {} MiB free",
        used / 1024,
        free as u64 / MIB
    );
}

/// Sends a request to the LLM and runs the validated action.
fn ask(ctx: &mut Context, request: &str) {
    let Some(agent) = ctx.agent.as_mut() else {
        println!("unknown command `{request}`; no LLM model is loaded (type `help`)");
        return;
    };
    let json = match agent.decide(request) {
        Ok(json) => json,
        Err(e) => return println!("error: {e}"),
    };
    let Ok(raw) = serde_json::from_str::<RawAction>(&json) else {
        return println!("the model's answer was not valid JSON");
    };
    let path = || raw.path.clone().filter(|p| fs::validate_name(p).is_ok());
    match raw.action.as_str() {
        "list_files" => list_files(),
        "read_file" => match path() {
            Some(p) => read_file(&p),
            None => println!("invalid file name"),
        },
        "write_file" => match (path(), raw.content.as_deref()) {
            (Some(p), Some(c)) if !fs::exists(&p) || confirm(&alloc::format!("Overwrite {p}?")) => {
                write_file(&p, c)
            }
            (Some(_), Some(_)) => println!("cancelled"),
            _ => println!("invalid write request"),
        },
        "delete_file" => match path() {
            Some(p) if confirm(&alloc::format!("Delete {p}?")) => delete_file(&p),
            Some(_) => println!("cancelled"),
            None => println!("invalid file name"),
        },
        "show_hardware" => ctx.info.print(),
        "show_memory" => show_memory(),
        "clear_screen" => print!("\x1b[2J\x1b[H"),
        "help" => print_help(),
        "say" => println!("{}", raw.text.as_deref().unwrap_or("")),
        "poweroff" if confirm("Power off?") => platform::poweroff(ctx.info.psci.as_deref()),
        "reboot" if confirm("Reboot?") => platform::reboot(ctx.info.psci.as_deref()),
        "poweroff" | "reboot" => println!("cancelled"),
        "list_disks" | "install" => {
            println!(
                "there are no disk drivers on this platform yet (see the roadmap in README.md)"
            )
        }
        other => println!("unknown action `{other}`"),
    }
}

fn selftest(ctx: &Context) {
    let mut failed = 0;
    let mut check = |name: &str, ok: bool| {
        println!("  {name:<24} {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    };
    let v: Vec<u64> = (0..100_000).collect();
    check(
        "heap allocation",
        v.iter().sum::<u64>() == 99_999 * 100_000 / 2,
    );
    let x: f32 = core::hint::black_box(2.0);
    check(
        "NEON floating point",
        (llm::math::sqrtf(x) * llm::math::sqrtf(x) - 2.0).abs() < 1e-6,
    );
    check("device tree", ctx.info.ram_mib > 0 && ctx.info.cores > 0);
    let t0 = platform::uptime_ms();
    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }
    check("generic timer", platform::uptime_ms() >= t0);
    let fs_ok = fs::write("selftest.tmp", b"abc").is_ok()
        && fs::read("selftest.tmp").as_deref() == Ok(&b"abc"[..])
        && fs::remove("selftest.tmp").is_ok();
    check("file system", fs_ok);
    if let Some(a) = &ctx.agent {
        let tok = &a.model.tokenizer;
        check(
            "tokenizer round trip",
            tok.decode(&tok.encode("llmos arm 123")) == b"llmos arm 123",
        );
    }
    if failed == 0 {
        println!("selftest: all checks passed");
    } else {
        println!("selftest: {failed} check(s) failed");
    }
}
