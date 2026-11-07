//! Compares the llm engine against Hugging Face `transformers`.
//!
//! Needs files that are too large to commit, so the tests skip (with a
//! message) when they are missing:
//!
//! * `models/test-vectors.json`: from `tools/make_test_vectors.py`,
//! * `models/smollm2-f32.model`: `tools/export_model.py --dtype f32 --out models/smollm2-f32.model`,
//! * `models/llmos.model`: `tools/export_model.py` (Q8, the default).
//!
//! Override paths with `LLMOS_TEST_VECTORS`, `LLMOS_F32_MODEL` and
//! `LLMOS_Q8_MODEL`. Run in release mode, the forward pass is slow otherwise:
//! `cargo test --release -p llm`.

use std::path::PathBuf;

use llm::constrained::{ActionDecoder, ActionSpec, FieldKind, FieldSpec, JSON_PREFIX};
use llm::{AlignedBuffer, Model};

/// Resolves a path from an env var or a default relative to the repo root.
fn path(var: &str, default: &str) -> PathBuf {
    std::env::var(var).map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(default)
    })
}

struct Case {
    prompt: String,
    token_ids: Vec<u32>,
    top_ids: Vec<u32>,
    top_logits: Vec<f32>,
    greedy: Vec<u32>,
}

/// Minimal parser for the JSON written by `make_test_vectors.py` (avoids a
/// serde dependency in this crate).
fn load_cases() -> Option<Vec<Case>> {
    let p = path("LLMOS_TEST_VECTORS", "models/test-vectors.json");
    let text = std::fs::read_to_string(&p).ok()?;
    let v: Json = parse_json(&text);
    let cases = v.get("cases").array();
    Some(
        cases
            .iter()
            .map(|c| Case {
                prompt: c.get("prompt").string(),
                token_ids: c
                    .get("token_ids")
                    .array()
                    .iter()
                    .map(|x| x.number() as u32)
                    .collect(),
                top_ids: c
                    .get("top_ids")
                    .array()
                    .iter()
                    .map(|x| x.number() as u32)
                    .collect(),
                top_logits: c
                    .get("top_logits")
                    .array()
                    .iter()
                    .map(|x| x.number() as f32)
                    .collect(),
                greedy: c
                    .get("greedy")
                    .array()
                    .iter()
                    .map(|x| x.number() as u32)
                    .collect(),
            })
            .collect(),
    )
}

fn load_model(var: &str, default: &str) -> Option<AlignedBuffer> {
    let p = path(var, default);
    match std::fs::read(&p) {
        Ok(bytes) => Some(AlignedBuffer::from_bytes(&bytes)),
        Err(_) => {
            eprintln!("skipping: {} not found", p.display());
            None
        }
    }
}

#[test]
fn tokenizer_matches_hugging_face() {
    let (Some(cases), Some(buf)) = (
        load_cases(),
        load_model("LLMOS_Q8_MODEL", "models/llmos.model"),
    ) else {
        eprintln!("skipping: reference files missing");
        return;
    };
    let model = Model::from_bytes(buf.as_bytes()).unwrap();
    for case in &cases {
        let ids = model.tokenizer.encode(&case.prompt);
        assert_eq!(ids, case.token_ids, "tokenization of {:?}", case.prompt);
        let decoded = model.tokenizer.decode(&ids);
        assert_eq!(String::from_utf8(decoded).unwrap(), case.prompt);
    }
}

#[test]
fn f32_logits_and_greedy_match_transformers() {
    let (Some(cases), Some(buf)) = (
        load_cases(),
        load_model("LLMOS_F32_MODEL", "models/smollm2-f32.model"),
    ) else {
        eprintln!("skipping: reference files missing");
        return;
    };
    let model = Model::from_bytes(buf.as_bytes()).unwrap();
    for case in &cases {
        let mut state = model.new_state(256);
        let all: Vec<u32> = (0..model.config.vocab_size as u32).collect();
        for (i, &t) in case.token_ids.iter().enumerate() {
            let l = if i + 1 == case.token_ids.len() {
                llm::Logits::All
            } else {
                llm::Logits::None
            };
            model.forward(&mut state, t, l);
        }
        for (&id, &expected) in case.top_ids.iter().zip(&case.top_logits) {
            let got = state.logits()[id as usize];
            assert!(
                (got - expected).abs() < 1e-2,
                "logit of {id}: {got} vs {expected} ({:?})",
                case.prompt
            );
        }
        assert_eq!(
            llm::constrained::argmax(state.logits(), &all),
            case.top_ids[0]
        );

        let mut state = model.new_state(256);
        let generated =
            llm::generate::greedy(&model, &mut state, &case.token_ids, case.greedy.len(), &[]);
        assert_eq!(
            generated, case.greedy,
            "greedy continuation of {:?}",
            case.prompt
        );
    }
}

#[test]
fn q8_greedy_stays_close_to_transformers() {
    let (Some(cases), Some(buf)) = (
        load_cases(),
        load_model("LLMOS_Q8_MODEL", "models/llmos.model"),
    ) else {
        eprintln!("skipping: reference files missing");
        return;
    };
    let model = Model::from_bytes(buf.as_bytes()).unwrap();
    let mut first_token_matches = 0;
    for case in &cases {
        let mut state = model.new_state(256);
        let generated = llm::generate::greedy(&model, &mut state, &case.token_ids, 4, &[]);
        let text = String::from_utf8_lossy(&model.tokenizer.decode(&generated)).into_owned();
        eprintln!("q8 {:?} -> {:?}", case.prompt, text);
        if generated.first() == case.greedy.first() {
            first_token_matches += 1;
        }
    }
    // Quantisation may flip near-ties; require nearly all first tokens.
    assert!(
        first_token_matches + 1 >= cases.len(),
        "only {first_token_matches}/{} first tokens matched",
        cases.len()
    );
}

static ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        name: "list_files",
        fields: &[],
    },
    ActionSpec {
        name: "write_file",
        fields: &[
            FieldSpec {
                name: "path",
                kind: FieldKind::Path,
            },
            FieldSpec {
                name: "content",
                kind: FieldKind::Text,
            },
        ],
    },
    ActionSpec {
        name: "show_hardware",
        fields: &[],
    },
    ActionSpec {
        name: "say",
        fields: &[FieldSpec {
            name: "text",
            kind: FieldKind::Text,
        }],
    },
];

#[test]
fn constrained_decoding_always_yields_valid_json() {
    let Some(buf) = load_model("LLMOS_Q8_MODEL", "models/llmos.model") else {
        return;
    };
    let model = Model::from_bytes(buf.as_bytes()).unwrap();
    let decoder = ActionDecoder::new(&model, ACTIONS);
    let system = "<|im_start|>system\nYou are the shell of an operating system. Reply with one JSON action: list_files, write_file(path, content), show_hardware, say(text).<|im_end|>\n";
    for (request, expected_action) in [
        (
            "create a file called notes.txt containing buy milk",
            "write_file",
        ),
        ("show me what hardware this computer has", "show_hardware"),
        ("list my files", "list_files"),
    ] {
        let prompt = format!(
            "{system}<|im_start|>user\n{request}<|im_end|>\n<|im_start|>assistant\n{JSON_PREFIX}"
        );
        let tokens = model.tokenizer.encode(&prompt);
        let mut state = model.new_state(512);
        let json = decoder.decode(&mut state, &tokens, &mut |_| {}).unwrap();
        eprintln!("{request:?} -> {json}");
        assert!(
            json.starts_with("{\"action\": \"") && json.ends_with("\"}"),
            "{json}"
        );
        let name = json[12..].split('"').next().unwrap();
        assert!(ACTIONS.iter().any(|a| a.name == name), "{json}");
        assert_eq!(name, expected_action, "{json}");
    }
}

// ---- A tiny JSON reader, just enough for the test vectors. ----

#[derive(Debug, Clone)]
enum Json {
    Null,
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(items) => items
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .unwrap_or(&Json::Null),
            _ => &Json::Null,
        }
    }
    fn array(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            _ => &[],
        }
    }
    fn number(&self) -> f64 {
        match self {
            Json::Num(n) => *n,
            _ => f64::NAN,
        }
    }
    fn string(&self) -> String {
        match self {
            Json::Str(s) => s.clone(),
            _ => String::new(),
        }
    }
}

fn parse_json(text: &str) -> Json {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    value(&chars, &mut i)
}

fn skip_ws(c: &[char], i: &mut usize) {
    while *i < c.len() && c[*i].is_whitespace() {
        *i += 1;
    }
}

fn value(c: &[char], i: &mut usize) -> Json {
    skip_ws(c, i);
    match c[*i] {
        '{' => {
            *i += 1;
            let mut items = Vec::new();
            loop {
                skip_ws(c, i);
                if c[*i] == '}' {
                    *i += 1;
                    break;
                }
                let Json::Str(k) = value(c, i) else {
                    panic!("key")
                };
                skip_ws(c, i);
                *i += 1; // ':'
                items.push((k, value(c, i)));
                skip_ws(c, i);
                if c[*i] == ',' {
                    *i += 1;
                }
            }
            Json::Obj(items)
        }
        '[' => {
            *i += 1;
            let mut items = Vec::new();
            loop {
                skip_ws(c, i);
                if c[*i] == ']' {
                    *i += 1;
                    break;
                }
                items.push(value(c, i));
                skip_ws(c, i);
                if c[*i] == ',' {
                    *i += 1;
                }
            }
            Json::Arr(items)
        }
        '"' => {
            *i += 1;
            let mut s = String::new();
            while c[*i] != '"' {
                if c[*i] == '\\' {
                    *i += 1;
                    match c[*i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'u' => {
                            let hex: String = c[*i + 1..*i + 5].iter().collect();
                            s.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                            *i += 4;
                        }
                        other => s.push(other),
                    }
                } else {
                    s.push(c[*i]);
                }
                *i += 1;
            }
            *i += 1;
            Json::Str(s)
        }
        'n' => {
            *i += 4;
            Json::Null
        }
        _ => {
            let start = *i;
            while *i < c.len() && (c[*i].is_ascii_digit() || "+-.eE".contains(c[*i])) {
                *i += 1;
            }
            Json::Num(c[start..*i].iter().collect::<String>().parse().unwrap())
        }
    }
}

#[test]
fn baked_prompt_cache_matches_fresh_computation() {
    let Some(buf) = load_model("LLMOS_Q8_MODEL", "models/llmos.model") else {
        return;
    };
    let model = Model::from_bytes(buf.as_bytes()).unwrap();
    let Some(cache) = llm::prompt_cache::PromptCache::find(buf.as_bytes(), &model) else {
        eprintln!("skipping: model has no baked prompt cache (run llm-bake)");
        return;
    };
    let prompt = model
        .tokenizer
        .encode(&llm::agent::prompt_prefix("CPU: test. RAM: 1 MiB."));
    let probe = model.tokenizer.encode(" hello");

    let mut cached = model.new_state(1024);
    let n = cache.apply(&mut cached, &prompt);
    assert_eq!(
        n,
        cache.tokens().len(),
        "cache should cover the static prompt"
    );
    let mut fresh = model.new_state(1024);
    for (state, start) in [(&mut cached, n), (&mut fresh, 0)] {
        for &t in &prompt[start..] {
            model.forward(state, t, llm::Logits::None);
        }
        model.forward(state, probe[0], llm::Logits::All);
    }
    let max_diff = cached
        .logits()
        .iter()
        .zip(fresh.logits())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(
        max_diff < 1e-3,
        "cached vs fresh logits differ by {max_diff}"
    );
}
