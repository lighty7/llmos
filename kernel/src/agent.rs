//! The LLM shell agent.
//!
//! Loads the model packed into the boot RAM disk and turns natural-language
//! requests into validated [`Action`]s:
//!
//! 1. The constant prompt prefix (instructions, examples, this machine's
//!    hardware summary) is run through the model once, on the first request,
//!    and its key/value cache is kept. If the model file carries a baked
//!    [`PromptCache`] for the machine-independent part, only the hardware
//!    summary has to be processed.
//! 2. Each request rewinds the context to the end of that prefix, appends the
//!    user's text and runs grammar-constrained decoding
//!    ([`llm::constrained`]), which can only produce well-formed JSON for a
//!    known action.
//! 3. The JSON is parsed and validated by [`Action::from_json`]; the shell
//!    then asks for confirmation if the action is destructive.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use llm::agent::{SHELL_ACTIONS, prompt_prefix, prompt_request};
use llm::constrained::{ActionDecoder, Event};
use llm::prompt_cache::PromptCache;
use llm::{FormatError, Logits, Model, State, TokenId};

use crate::actions::{Action, ActionError};
use crate::framebuffer::Color;
use crate::{MIB, console, print, println, time};

/// Context window allocated for the agent, in tokens.
const SEQ_LEN: usize = 1024;

/// Why a request could not be turned into an action.
#[derive(Debug)]
pub enum AgentError {
    /// Decoding failed (e.g. the request is too long).
    Decode(llm::constrained::DecodeError),
    /// The model's JSON did not validate.
    Invalid(ActionError),
}

impl core::fmt::Display for AgentError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AgentError::Decode(e) => write!(f, "{e}"),
            AgentError::Invalid(e) => write!(f, "{e}"),
        }
    }
}

/// A loaded model plus its inference state.
pub struct Agent {
    model: &'static Model<'static>,
    decoder: ActionDecoder<'static, 'static>,
    state: State,
    /// Tokens of the constant prompt prefix.
    prefix: Vec<TokenId>,
    /// Pre-computed cache for the start of `prefix`, if the model file has one.
    cache: Option<PromptCache<'static>>,
    /// Whether `prefix` has been processed into the key/value cache.
    prefix_ready: bool,
    /// Tokens processed and milliseconds spent by the last request.
    last_stats: Option<(usize, u64)>,
}

impl Agent {
    /// Loads the model from the RAM disk contents.
    ///
    /// # Errors
    ///
    /// Returns a [`FormatError`] if the RAM disk is not a valid model file.
    pub fn load(ramdisk: &'static [u8], machine_summary: &str) -> Result<Self, FormatError> {
        // The model lives for the rest of the uptime; leaking it gives the
        // decoder a `'static` reference without self-referential structs.
        let model: &'static Model<'static> = Box::leak(Box::new(Model::from_bytes(ramdisk)?));
        let decoder = ActionDecoder::new(model, SHELL_ACTIONS);
        let prefix = model.tokenizer.encode(&prompt_prefix(machine_summary));
        let state = model.new_state(SEQ_LEN);
        let cache = PromptCache::find(ramdisk, model);
        Ok(Self {
            model,
            decoder,
            state,
            prefix,
            cache,
            prefix_ready: false,
            last_stats: None,
        })
    }

    /// Whether the model file carries a usable baked prompt cache.
    pub fn has_prompt_cache(&self) -> bool {
        self.cache
            .as_ref()
            .is_some_and(|c| self.prefix.starts_with(c.tokens()))
    }

    /// One-line description for the boot log.
    pub fn summary(&self) -> String {
        let c = &self.model.config;
        alloc::format!(
            "{} ({:.0}M parameters, {:?} weights, {} layers), context {} tokens",
            self.model.name,
            c.parameter_count() as f64 / 1e6,
            c.weight_type,
            c.n_layers,
            self.state.seq_len
        )
    }

    /// Prints model details and the speed of the last request.
    pub fn print_info(&self) {
        let c = &self.model.config;
        println!("model:      {}", self.model.name);
        println!(
            "parameters: {:.1}M ({:?} weights)",
            c.parameter_count() as f64 / 1e6,
            c.weight_type
        );
        println!(
            "shape:      {} layers, dim {}, {} heads ({} kv), vocab {}",
            c.n_layers, c.dim, c.n_heads, c.n_kv_heads, c.vocab_size
        );
        println!(
            "context:    {} tokens ({} MiB of state), prompt prefix {} tokens{}",
            self.state.seq_len,
            self.model.state_bytes(self.state.seq_len) as u64 / MIB,
            self.prefix.len(),
            if self.prefix_ready { " (cached)" } else { "" }
        );
        if let Some((tokens, ms)) = self.last_stats {
            println!(
                "last ask:   {tokens} tokens in {ms} ms ({})",
                rate(tokens, ms)
            );
        }
    }

    /// Checks that `text` survives encoding and decoding unchanged.
    pub fn tokenizer_round_trips(&self, text: &str) -> bool {
        let tok = &self.model.tokenizer;
        tok.decode(&tok.encode(text)) == text.as_bytes()
    }

    /// Processes the constant prompt prefix if that has not happened yet.
    fn ensure_prefix(&mut self) -> Result<(), AgentError> {
        if self.prefix_ready {
            return Ok(());
        }
        self.state.rewind(0);
        let cached = self
            .cache
            .as_ref()
            .map_or(0, |c| c.apply(&mut self.state, &self.prefix));
        let rest = &self.prefix[cached..];
        println!(
            "reading the system prompt ({} tokens, {} from the baked cache, once per boot)...",
            self.prefix.len(),
            cached
        );
        let start = time::uptime_ms();
        let total = rest.len();
        self.decoder
            .prefill(&mut self.state, rest, Logits::None, &mut |e| {
                if let Event::Prompt { done, .. } = e {
                    if done % 16 == 0 || done == total {
                        print!("\r  {}%", done * 100 / total);
                    }
                }
            })
            .map_err(AgentError::Decode)?;
        let ms = time::uptime_ms() - start;
        println!(
            "\r  done in {}.{}s ({})",
            ms / 1000,
            (ms % 1000) / 100,
            rate(total, ms)
        );
        self.prefix_ready = true;
        Ok(())
    }

    /// Turns a natural-language request into a validated action, streaming
    /// the model's JSON to the console as it is produced.
    ///
    /// # Errors
    ///
    /// Fails if the request does not fit in the context or the model's
    /// answer does not validate.
    pub fn decide(&mut self, request: &str) -> Result<Action, AgentError> {
        self.ensure_prefix()?;
        self.state.rewind(self.prefix.len());
        let tokens = self.model.tokenizer.encode(&prompt_request(request));
        let start = time::uptime_ms();
        let start_pos = self.state.pos;

        console::set_color(Color::YELLOW);
        let result = self.decoder.decode(&mut self.state, &tokens, &mut |e| {
            if let Event::Output(text) = e {
                print!("{text}");
            }
        });
        console::set_color(Color::WHITE);
        println!();
        let json = result.map_err(AgentError::Decode)?;

        self.last_stats = Some((self.state.pos - start_pos, time::uptime_ms() - start));
        Action::from_json(&json).map_err(AgentError::Invalid)
    }
}

/// Formats a tokens-per-second rate.
fn rate(tokens: usize, ms: u64) -> String {
    if ms == 0 {
        return "n/a".into();
    }
    let tenths = tokens as u64 * 10_000 / ms;
    alloc::format!("{}.{} tokens/s", tenths / 10, tenths % 10)
}
