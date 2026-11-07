//! # llmos runner
//!
//! Host-side tool that builds and boots llmos. Run it with Cargo from the
//! repository root:
//!
//! ```text
//! cargo run                       # build + boot in a QEMU window (serial on this terminal)
//! cargo run -- run --headless     # no window: interact over the terminal only
//! cargo run -- image              # only build target/llmos-uefi.img
//! cargo run -- test               # automated end-to-end boot test (CI)
//! ```
//!
//! Options (for `run`, `image` and `test`):
//!
//! * `--release`: build the kernel with the release profile.
//! * `--model <path>`: model file packed into the boot RAM disk. Defaults to
//!   `models/llmos.model` if that file exists (see `models/README.md`).
//! * `--no-model`: boot without a model even if the default file exists.
//! * `--headless`: do not open a QEMU window.
//! * `--memory <MiB>`: guest RAM size (default 2048).
//!
//! Environment variables:
//!
//! * `OVMF_CODE` / `OVMF_VARS`: paths to the UEFI firmware images. By default
//!   common distribution locations are searched.
//! * `LLMOS_TEST_TIMEOUT`: seconds before `test` gives up (default 600).

use std::error::Error;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{env, fs, thread};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Default model location, relative to the repository root.
const DEFAULT_MODEL: &str = "models/llmos.model";

/// Exit code the kernel requests through QEMU's `isa-debug-exit` device on
/// success. QEMU reports `(value << 1) | 1`, so the kernel writes `0x10`.
const QEMU_EXIT_SUCCESS: i32 = (0x10 << 1) | 1;

/// What the runner was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Build the image and boot it interactively.
    Run,
    /// Only build the disk image.
    Image,
    /// Boot headless, drive the shell over serial and check the output.
    Test,
}

/// Parsed command-line options.
#[derive(Debug)]
struct Options {
    action: Action,
    release: bool,
    model: Option<PathBuf>,
    headless: bool,
    memory_mib: u32,
}

impl Options {
    /// Parses `std::env::args()`.
    fn parse() -> Result<Self> {
        let mut action = Action::Run;
        let mut release = false;
        let mut model = None;
        let mut no_model = false;
        let mut headless = false;
        let mut memory_mib = 2048;

        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "run" => action = Action::Run,
                "image" => action = Action::Image,
                "test" => action = Action::Test,
                "--release" => release = true,
                "--headless" => headless = true,
                "--no-model" => no_model = true,
                "--model" => {
                    model = Some(PathBuf::from(args.next().ok_or("--model needs a path")?))
                }
                "--memory" => memory_mib = args.next().ok_or("--memory needs a value")?.parse()?,
                "-h" | "--help" => {
                    println!(
                        "usage: cargo run -- [run|image|test] [--release] [--headless] [--model PATH] [--no-model] [--memory MIB]"
                    );
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument `{other}` (try --help)").into()),
            }
        }

        if model.is_none() && !no_model && Path::new(DEFAULT_MODEL).exists() {
            model = Some(PathBuf::from(DEFAULT_MODEL));
        }
        if no_model {
            model = None;
        }
        if action == Action::Test {
            headless = true;
        }
        Ok(Self {
            action,
            release,
            model,
            headless,
            memory_mib,
        })
    }
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<ExitCode> {
    let opts = Options::parse()?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    env::set_current_dir(&root)?;

    let kernel = build_kernel(&root, opts.release)?;
    let image = root.join("target").join("llmos-uefi.img");
    build_image(&kernel, opts.model.as_deref(), &image)?;
    println!("disk image: {}", image.display());

    match opts.action {
        Action::Image => Ok(ExitCode::SUCCESS),
        Action::Run => {
            let status = qemu_command(&root, &image, &opts)?.status()?;
            Ok(
                if status.success() || status.code() == Some(QEMU_EXIT_SUCCESS) {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(status.code().unwrap_or(1) as u8)
                },
            )
        }
        Action::Test => run_boot_test(&root, &image, &opts),
    }
}

/// Compiles the kernel crate for its bare-metal target and returns the path
/// of the resulting ELF file.
fn build_kernel(root: &Path, release: bool) -> Result<PathBuf> {
    let kernel_dir = root.join("kernel");
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.arg("build").current_dir(&kernel_dir);
    if release {
        cmd.arg("--release");
    }
    // Do not leak the host build's flags into the kernel build.
    for var in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_TARGET_DIR",
        "RUSTC_WORKSPACE_WRAPPER",
    ] {
        cmd.env_remove(var);
    }
    println!("building kernel...");
    if !cmd.status()?.success() {
        return Err("kernel build failed".into());
    }
    let profile = if release { "release" } else { "debug" };
    Ok(kernel_dir
        .join("target")
        .join("x86_64-llmos")
        .join(profile)
        .join("kernel"))
}

/// Packs the kernel (and optional model RAM disk) into a GPT disk image with
/// a FAT EFI system partition containing the UEFI bootloader.
fn build_image(kernel: &Path, model: Option<&Path>, image: &Path) -> Result<()> {
    let mut builder = bootloader::DiskImageBuilder::new(kernel.to_path_buf());
    if let Some(model) = model {
        if !model.exists() {
            return Err(format!("model file {} does not exist", model.display()).into());
        }
        println!("packing model {} into the boot RAM disk", model.display());
        builder.set_ramdisk(model.to_path_buf());
    }
    builder.create_uefi_image(image)?;
    Ok(())
}

/// Locates the OVMF UEFI firmware (code and variable-store templates).
fn find_ovmf() -> Result<(PathBuf, PathBuf)> {
    if let (Ok(code), Ok(vars)) = (env::var("OVMF_CODE"), env::var("OVMF_VARS")) {
        return Ok((code.into(), vars.into()));
    }
    const CANDIDATES: &[(&str, &str)] = &[
        (
            "/usr/share/OVMF/OVMF_CODE_4M.fd",
            "/usr/share/OVMF/OVMF_VARS_4M.fd",
        ),
        (
            "/usr/share/OVMF/OVMF_CODE.fd",
            "/usr/share/OVMF/OVMF_VARS.fd",
        ),
        (
            "/usr/share/edk2/ovmf/OVMF_CODE.fd",
            "/usr/share/edk2/ovmf/OVMF_VARS.fd",
        ),
        (
            "/usr/share/edk2/x64/OVMF_CODE.4m.fd",
            "/usr/share/edk2/x64/OVMF_VARS.4m.fd",
        ),
        (
            "/opt/homebrew/share/qemu/edk2-x86_64-code.fd",
            "/opt/homebrew/share/qemu/edk2-i386-vars.fd",
        ),
        (
            "/usr/local/share/qemu/edk2-x86_64-code.fd",
            "/usr/local/share/qemu/edk2-i386-vars.fd",
        ),
    ];
    CANDIDATES
        .iter()
        .find(|(code, vars)| Path::new(code).exists() && Path::new(vars).exists())
        .map(|(code, vars)| (PathBuf::from(code), PathBuf::from(vars)))
        .ok_or_else(|| {
            "OVMF firmware not found: install it (e.g. `sudo apt install ovmf`) \
             or set OVMF_CODE and OVMF_VARS"
                .into()
        })
}

/// Builds the QEMU command line for booting `image`.
fn qemu_command(root: &Path, image: &Path, opts: &Options) -> Result<Command> {
    let (ovmf_code, ovmf_vars) = find_ovmf()?;
    // UEFI writes to its variable store, so give QEMU a private copy.
    let vars_copy = root.join("target").join("OVMF_VARS.fd");
    fs::copy(&ovmf_vars, &vars_copy)?;

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.args(["-machine", "q35"])
        .args(["-m", &format!("{}M", opts.memory_mib)])
        .arg("-drive")
        .arg(format!(
            "if=pflash,format=raw,readonly=on,file={}",
            ovmf_code.display()
        ))
        .arg("-drive")
        .arg(format!("if=pflash,format=raw,file={}", vars_copy.display()))
        .arg("-drive")
        .arg(format!("format=raw,file={}", image.display()))
        .args(["-serial", "stdio"])
        // Lets the kernel power off QEMU with an exit code (used by `poweroff` and tests).
        .args(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"])
        .arg("-no-reboot");

    if Path::new("/dev/kvm").exists() && fs::File::open("/dev/kvm").is_ok() {
        cmd.args(["-accel", "kvm", "-cpu", "host"]);
    } else {
        // Software emulation; `max` exposes every CPU feature QEMU can emulate.
        cmd.args(["-cpu", "max"]);
    }
    if opts.headless {
        cmd.args(["-display", "none"]);
    }
    Ok(cmd)
}

/// Shell commands sent to the kernel during `cargo run -- test`, each paired
/// with a string that must appear in the output before the next is sent.
const TEST_SCRIPT: &[(&str, &str)] = &[
    ("selftest", "selftest: all checks passed"),
    (
        "write notes.txt hello from the test",
        "wrote 19 bytes to notes.txt",
    ),
    ("cat notes.txt", "hello from the test"),
    ("ls", "notes.txt"),
    ("rm notes.txt", "removed notes.txt"),
    ("hw", "\"pci_devices\""),
    ("poweroff", "powering off"),
];

/// Boots the image headless, waits for the shell prompt, runs
/// [`TEST_SCRIPT`] over the serial console and checks the responses.
fn run_boot_test(root: &Path, image: &Path, opts: &Options) -> Result<ExitCode> {
    let timeout = Duration::from_secs(
        env::var("LLMOS_TEST_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(600),
    );
    let mut child = qemu_command(root, image, opts)?
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;

    // Forward QEMU's serial output line by line through a channel so the
    // main thread can wait for expected text with a timeout.
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            println!("  | {line}");
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + timeout;
    let wait_for = |needle: &str| -> Result<()> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remaining) {
                Ok(line) if line.contains(needle) => return Ok(()),
                Ok(_) => {}
                Err(_) => return Err(format!("timed out waiting for `{needle}`").into()),
            }
        }
    };

    let result = (|| -> Result<()> {
        wait_for("shell ready")?;
        for (command, expected) in TEST_SCRIPT {
            println!("test: sending `{command}`");
            stdin.write_all(command.as_bytes())?;
            stdin.write_all(b"\r")?;
            stdin.flush()?;
            wait_for(expected)?;
        }
        Ok(())
    })();

    let status = match result {
        Ok(()) => {
            // `poweroff` was the last command; give QEMU a moment to exit.
            let start = Instant::now();
            loop {
                if let Some(status) = child.try_wait()? {
                    break Some(status);
                }
                if start.elapsed() > Duration::from_secs(30) {
                    break None;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        Err(err) => {
            eprintln!("test failed: {err}");
            None
        }
    };
    let _ = child.kill();

    match status {
        Some(status) if status.code() == Some(QEMU_EXIT_SUCCESS) => {
            println!("test: PASSED");
            Ok(ExitCode::SUCCESS)
        }
        other => {
            eprintln!("test: FAILED (QEMU status: {other:?})");
            Ok(ExitCode::FAILURE)
        }
    }
}
