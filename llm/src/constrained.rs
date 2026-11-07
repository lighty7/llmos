//! Grammar-constrained decoding of JSON actions.
//!
//! Small models are unreliable at free-form JSON. Instead of hoping, the
//! decoder *forces* the JSON structure and lets the model choose only where a
//! choice exists:
//!
//! ```text
//! {"action": "<one of the action names>", "<field>": "<string>", ...}
//!             ^^^^^^^^^^^^^^^^^^^^^^^^^^^              ^^^^^^^^
//!             model picks among allowed names          model writes, from an
//!             token by token (prefix trie)             allowed character set
//! ```
//!
//! Everything else (braces, quotes, field names) is fed to the model as fixed
//! text. The result is always well-formed JSON for a known action, so the
//! kernel's validator only has to check semantics (e.g. file-name rules).
//! Selection is greedy (argmax), which keeps behaviour deterministic.
//!
//! As a bonus, restricting the candidate tokens means only a subset of the
//! output classifier has to be evaluated ([`Logits::Subset`]).

use alloc::string::String;
use alloc::vec::Vec;

use crate::model::{Logits, Model, State};
use crate::tokenizer::TokenId;

/// What characters a string field may contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// A file name: `[A-Za-z0-9._-]`, at most 16 tokens.
    Path,
    /// Free text: printable ASCII except `"` and `\`, at most 40 tokens.
    Text,
}

impl FieldKind {
    fn max_tokens(self) -> usize {
        match self {
            FieldKind::Path => 16,
            FieldKind::Text => 40,
        }
    }

    fn allows(self, byte: u8) -> bool {
        match self {
            FieldKind::Path => byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'),
            FieldKind::Text => (0x20..0x7F).contains(&byte) && byte != b'"' && byte != b'\\',
        }
    }
}

/// A string field of an action.
#[derive(Debug, Clone, Copy)]
pub struct FieldSpec {
    /// JSON key.
    pub name: &'static str,
    /// Allowed contents.
    pub kind: FieldKind,
}

/// One action the model may choose.
#[derive(Debug, Clone, Copy)]
pub struct ActionSpec {
    /// Value of the `"action"` key.
    pub name: &'static str,
    /// Required string fields, in output order.
    pub fields: &'static [FieldSpec],
}

/// Text the model must be positioned after when [`ActionDecoder::decode`]
/// starts: the opening of the JSON object up to the action name.
pub const JSON_PREFIX: &str = "{\"action\": \"";

/// Progress events reported while decoding.
pub enum Event<'e> {
    /// `done` of `total` prompt tokens processed.
    Prompt {
        /// Tokens processed so far.
        done: usize,
        /// Tokens to process.
        total: usize,
    },
    /// A piece of the JSON output (forced or generated).
    Output(&'e str),
}

/// Errors from decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The prompt plus output would not fit in the context window.
    ContextFull,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::ContextFull => {
                f.write_str("the request is too long for the model's context window")
            }
        }
    }
}

/// Precomputed token sets for constrained decoding with one model.
pub struct ActionDecoder<'m, 'a> {
    model: &'m Model<'a>,
    actions: &'static [ActionSpec],
    /// Token sequence spelling each action name.
    name_tokens: Vec<Vec<TokenId>>,
    /// Tokens allowed inside a path / text string.
    path_tokens: Vec<TokenId>,
    text_tokens: Vec<TokenId>,
    /// Tokens that close a string: `"` optionally followed by `,`, `}` or
    /// whitespace.
    close_tokens: Vec<TokenId>,
}

impl<'m, 'a> ActionDecoder<'m, 'a> {
    /// Prepares a decoder for `actions`.
    pub fn new(model: &'m Model<'a>, actions: &'static [ActionSpec]) -> Self {
        let tok = &model.tokenizer;
        let name_tokens = actions.iter().map(|a| tok.encode(a.name)).collect();
        let (mut path_tokens, mut text_tokens, mut close_tokens) =
            (Vec::new(), Vec::new(), Vec::new());
        for id in 0..tok.vocab_size() as TokenId {
            if tok.is_special(id) {
                continue;
            }
            let bytes = tok.token_bytes(id);
            if bytes.is_empty() {
                continue;
            }
            if bytes.iter().all(|&b| FieldKind::Path.allows(b)) {
                path_tokens.push(id);
            }
            if bytes.iter().all(|&b| FieldKind::Text.allows(b)) {
                text_tokens.push(id);
            }
            if bytes[0] == b'"'
                && bytes[1..]
                    .iter()
                    .all(|b| matches!(b, b',' | b'}' | b' ' | b'\n'))
            {
                close_tokens.push(id);
            }
        }
        Self {
            model,
            actions,
            name_tokens,
            path_tokens,
            text_tokens,
            close_tokens,
        }
    }

    /// Runs prompt tokens through the model without computing logits
    /// (except, optionally, after the last one).
    ///
    /// # Errors
    ///
    /// [`DecodeError::ContextFull`] if the tokens do not fit.
    pub fn prefill(
        &self,
        state: &mut State,
        tokens: &[TokenId],
        last: Logits<'_>,
        on_event: &mut dyn FnMut(Event<'_>),
    ) -> Result<(), DecodeError> {
        if state.pos + tokens.len() > state.seq_len {
            return Err(DecodeError::ContextFull);
        }
        for (i, &t) in tokens.iter().enumerate() {
            let logits = if i + 1 == tokens.len() {
                last
            } else {
                Logits::None
            };
            self.model.forward(state, t, logits);
            on_event(Event::Prompt {
                done: i + 1,
                total: tokens.len(),
            });
        }
        Ok(())
    }

    /// Feeds fixed output text and reports it.
    fn force(
        &self,
        state: &mut State,
        text: &str,
        last: Logits<'_>,
        out: &mut String,
        on_event: &mut dyn FnMut(Event<'_>),
    ) -> Result<(), DecodeError> {
        let tokens = self.model.tokenizer.encode(text);
        if state.pos + tokens.len() > state.seq_len {
            return Err(DecodeError::ContextFull);
        }
        for (i, &t) in tokens.iter().enumerate() {
            let logits = if i + 1 == tokens.len() {
                last
            } else {
                Logits::None
            };
            self.model.forward(state, t, logits);
        }
        out.push_str(text);
        on_event(Event::Output(text));
        Ok(())
    }

    /// Token ids that may start the remaining candidate names at `depth`.
    fn name_choices(&self, alive: &[usize], depth: usize) -> Vec<TokenId> {
        let mut ids: Vec<TokenId> = alive
            .iter()
            .filter_map(|&i| self.name_tokens[i].get(depth).copied())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Decodes one action.
    ///
    /// `prompt` is fed starting at `state.pos` and must end with
    /// [`JSON_PREFIX`] (the assistant turn opening the JSON object). The
    /// returned string is the complete JSON object.
    ///
    /// # Errors
    ///
    /// [`DecodeError::ContextFull`] if the context window overflows.
    pub fn decode(
        &self,
        state: &mut State,
        prompt: &[TokenId],
        on_event: &mut dyn FnMut(Event<'_>),
    ) -> Result<String, DecodeError> {
        let model = self.model;
        let mut out = String::from(JSON_PREFIX);

        // 1. Pick the action name through the prefix trie of name tokens.
        let mut alive: Vec<usize> = (0..self.actions.len()).collect();
        let mut choices = self.name_choices(&alive, 0);
        self.prefill(state, prompt, Logits::Subset(&choices), on_event)?;
        on_event(Event::Output(JSON_PREFIX));
        let mut depth = 0;
        let chosen = loop {
            let logits = state.logits();
            let best = argmax(logits, &choices);
            alive.retain(|&i| self.name_tokens[i].get(depth) == Some(&best));
            depth += 1;
            if let Some(&done) = alive.iter().find(|&&i| self.name_tokens[i].len() == depth) {
                // Feed the final name token; its logits are not needed.
                self.step(state, best, Logits::None)?;
                break done;
            }
            choices = self.name_choices(&alive, depth);
            self.step(state, best, Logits::Subset(&choices))?;
        };
        let spec = self.actions[chosen];
        out.push_str(spec.name);
        on_event(Event::Output(spec.name));

        // 2. Each field: forced key, generated value.
        for field in spec.fields {
            let key = alloc::format!("\", \"{}\": \"", field.name);
            let allowed = match field.kind {
                FieldKind::Path => &self.path_tokens,
                FieldKind::Text => &self.text_tokens,
            };
            let mut candidates: Vec<TokenId> = allowed.clone();
            self.force(state, &key, Logits::Subset(&candidates), &mut out, on_event)?;
            // Closing the string is allowed only once it is non-empty.
            candidates.extend_from_slice(&self.close_tokens);
            let mut value: Vec<TokenId> = Vec::new();
            for n in 0..field.kind.max_tokens() {
                let logits = state.logits();
                let best = if n == 0 {
                    argmax(logits, allowed)
                } else {
                    argmax(logits, &candidates)
                };
                if self.close_tokens.contains(&best) {
                    break;
                }
                value.push(best);
                // Greedy decoding of small models easily loops ("Good morning!
                // Good morning! ..."). Stop as soon as the value starts
                // repeating itself.
                if repeats(&value) {
                    break;
                }
                let text = core::str::from_utf8(model.tokenizer.token_bytes(best)).unwrap_or("");
                out.push_str(text);
                on_event(Event::Output(text));
                self.step(state, best, Logits::Subset(&candidates))?;
            }
        }

        // 3. Close the object.
        self.force(state, "\"}", Logits::None, &mut out, on_event)?;
        Ok(out)
    }

    fn step(
        &self,
        state: &mut State,
        token: TokenId,
        logits: Logits<'_>,
    ) -> Result<(), DecodeError> {
        if state.pos >= state.seq_len {
            return Err(DecodeError::ContextFull);
        }
        self.model.forward(state, token, logits);
        Ok(())
    }
}

/// Length of the token window checked by [`repeats`].
const REPEAT_WINDOW: usize = 3;

/// Whether the last [`REPEAT_WINDOW`] tokens of `tokens` already occurred
/// earlier in it (the new token would start a loop).
fn repeats(tokens: &[TokenId]) -> bool {
    if tokens.len() <= REPEAT_WINDOW {
        return false;
    }
    let tail = &tokens[tokens.len() - REPEAT_WINDOW..];
    tokens[..tokens.len() - 1]
        .windows(REPEAT_WINDOW)
        .take(tokens.len() - REPEAT_WINDOW)
        .any(|w| w == tail)
}

/// Highest-scoring token among `ids` (the first one on ties).
pub fn argmax(logits: &[f32], ids: &[TokenId]) -> TokenId {
    let mut best = ids[0];
    for &id in &ids[1..] {
        if logits[id as usize] > logits[best as usize] {
            best = id;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_repetition() {
        assert!(!repeats(&[1, 2, 3, 4, 5]));
        assert!(repeats(&[1, 2, 3, 9, 1, 2, 3]));
        assert!(!repeats(&[1, 2, 3]));
        assert!(repeats(&[7, 7, 7, 7]));
    }
}
