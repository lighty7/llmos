//! Byte-level BPE tokenizer (GPT-2 / Llama-3 / SmolLM style).
//!
//! Encoding follows the Hugging Face `tokenizers` pipeline used by SmolLM2:
//!
//! 1. split out *special tokens* such as `<|im_start|>` verbatim,
//! 2. pre-tokenise: isolate every digit, then apply the GPT-2 regex
//!    `'s|'t|'re|'ve|'m|'ll|'d| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+(?!\S)|\s+`,
//! 3. run byte-pair merges, lowest merge rank first, inside each piece.
//!
//! The exporter stores every vocabulary entry as its *raw bytes* (the GPT-2
//! byte-to-unicode mapping is undone), so decoding is simple concatenation.
//!
//! Section layout (see `docs/model-format.md`):
//!
//! ```text
//! u32 n_tokens, u32 n_merges
//! n_tokens × { u8 flags (bit 0 = special), u32 len, len bytes }
//! n_merges × { u32 left, u32 right, u32 result }   // in rank order
//! ```

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::format::{FormatError, Reader};

/// A token id.
pub type TokenId = u32;

/// Vocabulary plus merge rules.
pub struct Tokenizer<'a> {
    /// Bytes of each token, indexed by id.
    vocab: Vec<&'a [u8]>,
    /// Token id of each single byte, if the vocabulary has one. Some
    /// vocabularies omit bytes that never occur in UTF-8 text (e.g. `0xFF`).
    byte_tokens: [Option<TokenId>; 256],
    /// `(left, right) -> (rank, merged)`.
    merges: BTreeMap<(TokenId, TokenId), (u32, TokenId)>,
    /// Special tokens, longest first, matched verbatim before BPE.
    specials: Vec<(&'a [u8], TokenId)>,
}

impl<'a> Tokenizer<'a> {
    /// Parses the tokenizer section of a model file.
    ///
    /// # Errors
    ///
    /// Returns [`FormatError::BadTokenizer`] if the section is inconsistent,
    /// e.g. a byte has no single-byte token or a merge refers to an unknown id.
    pub fn from_bytes(section: &'a [u8]) -> Result<Self, FormatError> {
        let mut r = Reader::new(section);
        let n_tokens = r.u32()? as usize;
        let n_merges = r.u32()? as usize;

        let mut vocab = Vec::with_capacity(n_tokens);
        let mut specials = Vec::new();
        let mut byte_tokens = [None; 256];
        for id in 0..n_tokens {
            let flags = r.u8()?;
            let len = r.u32()? as usize;
            let bytes = r.bytes(len)?;
            let id = id as TokenId;
            if flags & 1 != 0 {
                specials.push((bytes, id));
            } else if bytes.len() == 1 && byte_tokens[bytes[0] as usize].is_none() {
                byte_tokens[bytes[0] as usize] = Some(id);
            }
            vocab.push(bytes);
        }
        if byte_tokens.iter().all(Option::is_none) {
            return Err(FormatError::BadTokenizer(
                "vocabulary has no single-byte tokens",
            ));
        }
        specials.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let mut merges = BTreeMap::new();
        for rank in 0..n_merges {
            let (left, right, result) = (r.u32()?, r.u32()?, r.u32()?);
            if [left, right, result]
                .iter()
                .any(|&t| t as usize >= n_tokens)
            {
                return Err(FormatError::BadTokenizer(
                    "merge refers to an unknown token",
                ));
            }
            // Keep the first (highest-priority) rule if a pair repeats.
            merges.entry((left, right)).or_insert((rank as u32, result));
        }
        Ok(Self {
            vocab,
            byte_tokens,
            merges,
            specials,
        })
    }

    /// Number of tokens in the vocabulary.
    pub fn vocab_size(&self) -> usize {
        self.vocab.len()
    }

    /// Raw bytes of token `id` (empty for out-of-range ids).
    pub fn token_bytes(&self, id: TokenId) -> &'a [u8] {
        self.vocab.get(id as usize).copied().unwrap_or(&[])
    }

    /// Whether `id` is a special (control) token.
    pub fn is_special(&self, id: TokenId) -> bool {
        self.specials.iter().any(|&(_, s)| s == id)
    }

    /// Looks up a special token by its text, e.g. `"<|im_end|>"`.
    pub fn special_id(&self, text: &str) -> Option<TokenId> {
        self.specials
            .iter()
            .find(|(b, _)| *b == text.as_bytes())
            .map(|&(_, id)| id)
    }

    /// Concatenates the bytes of `tokens`.
    pub fn decode(&self, tokens: &[TokenId]) -> Vec<u8> {
        tokens
            .iter()
            .flat_map(|&t| self.token_bytes(t).iter().copied())
            .collect()
    }

    /// Encodes `text`, recognising special tokens written literally in it.
    pub fn encode(&self, text: &str) -> Vec<TokenId> {
        let mut out = Vec::new();
        let bytes = text.as_bytes();
        let mut start = 0; // Start of the pending ordinary text.
        let mut i = 0;
        while i < bytes.len() {
            if let Some(&(sbytes, id)) = self
                .specials
                .iter()
                .find(|(s, _)| bytes[i..].starts_with(s))
            {
                self.encode_ordinary(&text[start..i], &mut out);
                out.push(id);
                i += sbytes.len();
                start = i;
            } else {
                i += 1;
            }
        }
        self.encode_ordinary(&text[start..], &mut out);
        out
    }

    /// Encodes text that contains no special tokens.
    fn encode_ordinary(&self, text: &str, out: &mut Vec<TokenId>) {
        for piece in pre_tokenize(text) {
            self.bpe(piece.as_bytes(), out);
        }
    }

    /// Applies byte-pair merges to one pre-tokenised piece. Bytes without a
    /// token are dropped, as Hugging Face does for BPE without `unk_token`.
    fn bpe(&self, piece: &[u8], out: &mut Vec<TokenId>) {
        let mut tokens: Vec<TokenId> = piece
            .iter()
            .filter_map(|&b| self.byte_tokens[b as usize])
            .collect();
        loop {
            // Find the adjacent pair with the lowest merge rank.
            let best = tokens
                .windows(2)
                .enumerate()
                .filter_map(|(i, w)| {
                    self.merges
                        .get(&(w[0], w[1]))
                        .map(|&(rank, merged)| (rank, i, merged))
                })
                .min();
            let Some((_, i, merged)) = best else { break };
            tokens[i] = merged;
            tokens.remove(i + 1);
        }
        out.extend_from_slice(&tokens);
    }
}

/// Character classes used by the GPT-2 pre-tokenisation regex.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Letter,
    Number,
    Space,
    Other,
}

fn class(c: char) -> Class {
    if c.is_alphabetic() {
        Class::Letter
    } else if c.is_numeric() {
        Class::Number
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Other
    }
}

/// Splits `text` into pieces: digits individually, then the GPT-2 regex.
pub fn pre_tokenize(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    // Step 1 (`Digits { individual_digits: true }`): every ASCII digit is
    // its own piece; the text between digits is split further by step 2.
    let mut seg_start = 0;
    for (i, c) in text.char_indices() {
        if c.is_ascii_digit() {
            gpt2_split(&text[seg_start..i], &mut pieces);
            pieces.push(&text[i..i + 1]);
            seg_start = i + 1;
        }
    }
    gpt2_split(&text[seg_start..], &mut pieces);
    pieces
}

/// Step 2: the GPT-2 pre-tokenisation regex, implemented by hand.
fn gpt2_split<'t>(text: &'t str, out: &mut Vec<&'t str>) {
    const CONTRACTIONS: [&str; 7] = ["'s", "'t", "'re", "'ve", "'m", "'ll", "'d"];
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let byte_at = |idx: usize| chars.get(idx).map_or(text.len(), |&(b, _)| b);
    let mut i = 0;
    while i < chars.len() {
        let rest = &text[chars[i].0..];
        if let Some(c) = CONTRACTIONS.iter().find(|c| rest.starts_with(**c)) {
            out.push(&rest[..c.len()]);
            i += c.chars().count();
            continue;
        }
        // ` ?X+` for X in {letters, numbers, other}: an optional single
        // ASCII space followed by a run of one class.
        let lead = usize::from(chars[i].1 == ' ');
        if let Some(&(_, c)) = chars.get(i + lead) {
            let cls = class(c);
            if cls != Class::Space {
                let mut j = i + lead;
                while j < chars.len() && class(chars[j].1) == cls {
                    j += 1;
                }
                out.push(&text[chars[i].0..byte_at(j)]);
                i = j;
                continue;
            }
        }
        // `\s+(?!\S)|\s+`: a whitespace run; if non-space text follows, the
        // last whitespace character is left to prefix the next piece.
        let mut j = i;
        while j < chars.len() && class(chars[j].1) == Class::Space {
            j += 1;
        }
        if j < chars.len() && j - i > 1 {
            j -= 1;
        }
        out.push(&text[chars[i].0..byte_at(j)]);
        i = j;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_tokenize_matches_gpt2_rules() {
        assert_eq!(pre_tokenize("Hello world"), ["Hello", " world"]);
        assert_eq!(
            pre_tokenize("it's 2024!"),
            ["it", "'s", " ", "2", "0", "2", "4", "!"]
        );
        assert_eq!(pre_tokenize("a  b"), ["a", " ", " b"]);
        assert_eq!(pre_tokenize("x\n\ny"), ["x", "\n", "\n", "y"]);
        assert_eq!(pre_tokenize("end.  "), ["end", ".", "  "]);
        assert_eq!(
            pre_tokenize("{\"a\": 1}"),
            ["{\"", "a", "\":", " ", "1", "}"]
        );
    }

    /// Builds a tokenizer section: bytes 0..=255, "ab", "abc", one special.
    fn tiny_section() -> Vec<u8> {
        let mut tokens: Vec<(u8, Vec<u8>)> = (0..=255u8).map(|b| (0, alloc::vec![b])).collect();
        tokens.push((0, b"ab".to_vec())); // 256
        tokens.push((0, b"abc".to_vec())); // 257
        tokens.push((1, b"<|end|>".to_vec())); // 258
        let merges = [(b'a' as u32, b'b' as u32, 256u32), (256, b'c' as u32, 257)];
        let mut s = Vec::new();
        s.extend_from_slice(&(tokens.len() as u32).to_le_bytes());
        s.extend_from_slice(&(merges.len() as u32).to_le_bytes());
        for (flags, bytes) in &tokens {
            s.push(*flags);
            s.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            s.extend_from_slice(bytes);
        }
        for (l, r, m) in merges {
            for v in [l, r, m] {
                s.extend_from_slice(&v.to_le_bytes());
            }
        }
        s
    }

    #[test]
    fn bpe_merges_and_special_tokens() {
        let section = tiny_section();
        let tok = Tokenizer::from_bytes(&section).unwrap();
        assert_eq!(tok.encode("abc"), [257]);
        assert_eq!(tok.encode("abd"), [256, b'd' as u32]);
        assert_eq!(tok.encode("ab<|end|>c"), [256, 258, b'c' as u32]);
        assert_eq!(tok.decode(&[257, 258]), b"abc<|end|>");
        assert_eq!(tok.special_id("<|end|>"), Some(258));
        assert!(tok.is_special(258));
    }
}
