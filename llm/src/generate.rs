//! Unconstrained greedy text generation.
//!
//! Used for diagnostics (the kernel's `model` self-check) and for comparing
//! this engine against reference implementations in tests.

use alloc::vec::Vec;

use crate::constrained::argmax;
use crate::model::{Logits, Model, State};
use crate::tokenizer::TokenId;

/// Feeds `prompt`, then generates up to `max_new` tokens by always taking
/// the most likely next token. Stops early at any token in `stop`.
/// Returns the generated token ids (excluding the stop token).
///
/// # Panics
///
/// Panics if `prompt` is empty or the context window overflows.
pub fn greedy(
    model: &Model<'_>,
    state: &mut State,
    prompt: &[TokenId],
    max_new: usize,
    stop: &[TokenId],
) -> Vec<TokenId> {
    assert!(!prompt.is_empty(), "prompt must not be empty");
    let all: Vec<TokenId> = (0..model.config.vocab_size as TokenId).collect();
    for (i, &t) in prompt.iter().enumerate() {
        let logits = if i + 1 == prompt.len() {
            Logits::All
        } else {
            Logits::None
        };
        model.forward(state, t, logits);
    }
    let mut out = Vec::new();
    for _ in 0..max_new {
        let next = argmax(state.logits(), &all);
        if stop.contains(&next) {
            break;
        }
        out.push(next);
        model.forward(state, next, Logits::All);
    }
    out
}
