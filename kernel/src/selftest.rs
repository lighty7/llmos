//! In-kernel self-tests, run with the `selftest` shell command.
//!
//! Bare-metal code cannot use `cargo test`, so these checks exercise the
//! real hardware paths (interrupts, paging, heap, devices) on the running
//! system. The automated boot test (`cargo run -- test`) runs them and looks
//! for the final "all checks passed" line.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use crate::actions::Action;
use crate::framebuffer::Color;
use crate::shell::Context;
use crate::{console, fs, interrupts, keyboard, print, println, time};

/// Runs every check, printing one line per check.
pub fn run(ctx: &Context) {
    let checks: &[(&str, fn(&Context) -> Result<(), &'static str>)] = &[
        ("heap allocation", heap),
        ("breakpoint exception", breakpoint),
        ("timer interrupts", timer),
        ("keyboard decoding", keyboard_map),
        ("SSE floating point", sse),
        ("ACPI tables", acpi),
        ("PCI enumeration", pci),
        ("file system", file_system),
        ("action validation", action_validation),
        ("LLM model", model),
    ];
    let mut failed = 0;
    for (name, check) in checks {
        print!("  {name:<24} ");
        match check(ctx) {
            Ok(()) => {
                console::set_color(Color::GREEN);
                println!("ok");
            }
            Err(why) => {
                failed += 1;
                console::set_color(Color::RED);
                println!("FAILED: {why}");
            }
        }
        console::set_color(Color::WHITE);
    }
    if failed == 0 {
        println!("selftest: all checks passed");
    } else {
        println!("selftest: {failed} check(s) failed");
    }
}

fn ensure(cond: bool, why: &'static str) -> Result<(), &'static str> {
    if cond { Ok(()) } else { Err(why) }
}

fn heap(_: &Context) -> Result<(), &'static str> {
    let boxed = Box::new(41u64);
    let big: Vec<u64> = (0..1_000_000).collect();
    ensure(*boxed + 1 == 42, "Box value corrupted")?;
    ensure(
        big.iter().sum::<u64>() == 999_999 * 1_000_000 / 2,
        "large Vec corrupted",
    )
}

fn breakpoint(_: &Context) -> Result<(), &'static str> {
    let before = interrupts::BREAKPOINTS.load(Ordering::Relaxed);
    x86_64::instructions::interrupts::int3();
    ensure(
        interrupts::BREAKPOINTS.load(Ordering::Relaxed) == before + 1,
        "int3 handler did not run",
    )
}

fn timer(_: &Context) -> Result<(), &'static str> {
    let before = time::ticks();
    time::sleep_ms(50);
    ensure(time::ticks() > before, "timer did not tick")
}

fn keyboard_map(_: &Context) -> Result<(), &'static str> {
    // 0x1E = 'a' make code, 0x2A = left shift make, 0xAA = left shift break.
    ensure(
        keyboard::translate_for_test(0x1E) == Some(b'a'),
        "'a' not decoded",
    )?;
    ensure(
        keyboard::translate_for_test(0x2A).is_none(),
        "shift produced a character",
    )?;
    let shifted = keyboard::translate_for_test(0x1E);
    keyboard::translate_for_test(0xAA);
    ensure(shifted == Some(b'A'), "shift+'a' not upper case")
}

fn sse(_: &Context) -> Result<(), &'static str> {
    // `black_box` stops the compiler from folding this at build time.
    let x: f32 = core::hint::black_box(2.0);
    let y = llm::math::sqrtf(x) * llm::math::sqrtf(x);
    ensure((y - 2.0).abs() < 1e-6, "float math is wrong")
}

fn acpi(ctx: &Context) -> Result<(), &'static str> {
    let acpi = ctx.inventory.acpi.as_ref().ok_or("no valid ACPI tables")?;
    ensure(
        acpi.tables.iter().any(|t| t == "APIC"),
        "no MADT (APIC) table",
    )?;
    ensure(acpi.processors > 0, "MADT lists no processors")
}

fn pci(ctx: &Context) -> Result<(), &'static str> {
    let devices = &ctx.inventory.pci_devices;
    ensure(!devices.is_empty(), "no PCI devices found")?;
    ensure(
        devices
            .iter()
            .any(|d| d.class == 0x06 && d.subclass == 0x00),
        "no host bridge",
    )
}

fn file_system(_: &Context) -> Result<(), &'static str> {
    fs::write("selftest.tmp", b"abc").map_err(|_| "write failed")?;
    ensure(
        fs::read("selftest.tmp").as_deref() == Ok(&b"abc"[..]),
        "read back mismatch",
    )?;
    fs::remove("selftest.tmp").map_err(|_| "remove failed")?;
    ensure(fs::write("bad/name", b"").is_err(), "invalid name accepted")
}

fn action_validation(_: &Context) -> Result<(), &'static str> {
    let ok = Action::from_json(r#"{"action": "write_file", "path": "a.txt", "content": "hi"}"#);
    ensure(
        matches!(ok, Ok(Action::WriteFile { .. })),
        "valid action rejected",
    )?;
    ensure(
        Action::from_json(r#"{"action": "format_disk"}"#).is_err(),
        "unknown action accepted",
    )?;
    ensure(
        Action::from_json(r#"{"action": "read_file", "path": "../etc"}"#).is_err(),
        "bad path accepted",
    )?;
    ensure(Action::from_json("not json").is_err(), "garbage accepted")
}

fn model(ctx: &Context) -> Result<(), &'static str> {
    // Every action the model can choose must be one the kernel understands.
    for spec in llm::agent::SHELL_ACTIONS {
        let mut json = alloc::format!("{{\"action\": \"{}\"", spec.name);
        for field in spec.fields {
            json.push_str(&alloc::format!(", \"{}\": \"x.txt\"", field.name));
        }
        json.push('}');
        ensure(
            Action::from_json(&json).is_ok(),
            "agent action list out of sync with the kernel",
        )?;
    }
    // Not having a model is a valid configuration, not a failure.
    match &ctx.agent {
        Some(agent) => ensure(
            agent.tokenizer_round_trips("llmos self-test 123!"),
            "tokenizer round trip failed",
        ),
        None => Ok(()),
    }
}
