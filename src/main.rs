//! # llmos runner
//!
//! Host-side tool that builds and boots llmos. Run it with Cargo from the
//! repository root:
//!
//! ```text
//! cargo run                       # build + boot in a QEMU window (serial on this terminal)
//! cargo run -- run --headless     # no window: interact over the terminal only
//! cargo run -- run --installed    # boot the disk llmos was installed onto
//! cargo run -- image              # only build target/llmos-uefi.img
//! cargo run -- test               # automated end-to-end test (CI)
//! ```
//!
//! The boot image is attached as a virtio disk (`vd0`). Unless `--no-disk` is
//! given, a second, initially empty virtio disk (`vd1`, `target/disk.img`)
//! is attached as an installation target; delete the file to start afresh.
//!
//! Options (for `run`, `image` and `test`):
//!
//! * `--release`: build the kernel with the release profile.
//! * `--model <path>`: model file packed into the boot RAM disk. Defaults to
//!   `models/llmos.model` if that file exists (see `models/README.md`).
//! * `--no-model`: boot without a model even if the default file exists.
//! * `--headless`: do not open a QEMU window.
//! * `--memory <MiB>`: guest RAM size (default 2048).
//! * `--no-disk`: do not attach the installation target disk.
//! * `--installed`: boot only from the installation target disk.
//!
//! Environment variables:
//!
//! * `OVMF_CODE` / `OVMF_VARS`: paths to the UEFI firmware images. By default
//!   common distribution locations are searched.
//! * `LLMOS_TEST_TIMEOUT`: seconds each test stage may take (default 600).

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

/// Size of a freshly created installation target disk.
const TARGET_DISK_BYTES: u64 = 512 * 1024 * 1024;

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
    target_disk: bool,
    installed: bool,
}

impl Options {
    /// Parses `std::env::args()`.
    fn parse() -> Result<Self> {
        let mut opts = Options {
            action: Action::Run,
            release: false,
            model: None,
            headless: false,
            memory_mib: 2048,
            target_disk: true,
            installed: false,
        };
        let mut no_model = false;
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "run" => opts.action = Action::Run,
                "image" => opts.action = Action::Image,
                "test" => opts.action = Action::Test,
                "--release" => opts.release = true,
                "--headless" => opts.headless = true,
                "--no-model" => no_model = true,
                "--no-disk" => opts.target_disk = false,
                "--installed" => opts.installed = true,
                "--model" => {
                    opts.model = Some(PathBuf::from(args.next().ok_or("--model needs a path")?))
                }
                "--memory" => {
                    opts.memory_mib = args.next().ok_or("--memory needs a value")?.parse()?
                }
                "-h" | "--help" => {
                    println!(
                        "usage: cargo run -- [run|image|test] [--release] [--headless] \
                         [--model PATH] [--no-model] [--memory MIB] [--no-disk] [--installed]"
                    );
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument `{other}` (try --help)").into()),
            }
        }
        if opts.model.is_none() && !no_model && Path::new(DEFAULT_MODEL).exists() {
            opts.model = Some(PathBuf::from(DEFAULT_MODEL));
        }
        if no_model {
            opts.model = None;
        }
        if opts.action == Action::Test {
            opts.headless = true;
        }
        Ok(opts)
    }
}

/// Disks to attach to the virtual machine, in order (`vd0`, `vd1`, ...).
struct Disks(Vec<PathBuf>);

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
    let target_dir = root.join("target");
    let target_disk = target_dir.join("disk.img");

    if opts.action == Action::Run && opts.installed {
        if !target_disk.exists() {
            return Err(
                "target/disk.img does not exist; boot normally and run `install` first".into(),
            );
        }
        return run_interactive(&root, &Disks(vec![target_disk]), &opts);
    }

    let kernel = build_kernel(&root, opts.release)?;
    let image = target_dir.join("llmos-uefi.img");
    build_image(&kernel, opts.model.as_deref(), &image)?;
    println!("disk image: {}", image.display());

    match opts.action {
        Action::Image => Ok(ExitCode::SUCCESS),
        Action::Run => {
            let mut disks = vec![image];
            if opts.target_disk {
                if !target_disk.exists() {
                    create_empty_disk(&target_disk)?;
                }
                disks.push(target_disk);
            }
            run_interactive(&root, &Disks(disks), &opts)
        }
        Action::Test => run_tests(&root, &image, &opts),
    }
}

/// Boots the machine with the terminal attached to the serial console.
fn run_interactive(root: &Path, disks: &Disks, opts: &Options) -> Result<ExitCode> {
    let status = qemu_command(root, disks, opts)?.status()?;
    Ok(
        if status.success() || status.code() == Some(QEMU_EXIT_SUCCESS) {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(status.code().unwrap_or(1) as u8)
        },
    )
}

/// Creates a sparse, zero-filled disk image.
fn create_empty_disk(path: &Path) -> Result<()> {
    let file = fs::File::create(path)?;
    file.set_len(TARGET_DISK_BYTES)?;
    println!(
        "created empty disk {} ({} MiB)",
        path.display(),
        TARGET_DISK_BYTES >> 20
    );
    Ok(())
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

/// Builds the QEMU command line booting from the first of `disks`.
fn qemu_command(root: &Path, disks: &Disks, opts: &Options) -> Result<Command> {
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
        .args(["-serial", "stdio"])
        // Lets the kernel power off QEMU with an exit code (used by `poweroff` and tests).
        .args(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"])
        .arg("-no-reboot");
    for (i, disk) in disks.0.iter().enumerate() {
        cmd.arg("-drive")
            .arg(format!(
                "if=none,id=disk{i},format=raw,file={}",
                disk.display()
            ))
            .arg("-device")
            .arg(format!(
                "virtio-blk-pci,drive=disk{i}{}",
                if i == 0 { ",bootindex=0" } else { "" }
            ));
    }

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

/// One step of a test script: text to type (`None`: just wait) and text that
/// must then appear on the console.
type Step = (Option<&'static str>, &'static str);

/// Stage 1: boot the installer image with an empty disk attached.
const STAGE1: &[Step] = &[
    (None, "shell ready"),
    (Some("selftest"), "selftest: all checks passed"),
    (
        Some("write notes.txt hello from the test"),
        "wrote 19 bytes to notes.txt",
    ),
    (Some("cat notes.txt"), "hello from the test"),
    (Some("ls"), "notes.txt"),
    (Some("rm notes.txt"), "removed notes.txt"),
    (Some("hw"), "\"pci_devices\""),
    (Some("disks"), "vd1"),
    (Some("install"), "install llmos onto vd1"),
    (Some("y"), "llmos is installed on vd1"),
    (
        Some("write kept.txt this survives a reboot"),
        "wrote 22 bytes to kept.txt",
    ),
    (Some("poweroff"), "powering off"),
];

/// Stage 2: boot from the freshly installed disk alone.
const STAGE2: &[Step] = &[
    (None, "shell ready"),
    (Some("cat kept.txt"), "this survives a reboot"),
    (Some("disks"), "llmos installed"),
    (Some("poweroff"), "powering off"),
];

/// Runs both test stages: install onto a fresh disk, then boot from it.
fn run_tests(root: &Path, image: &Path, opts: &Options) -> Result<ExitCode> {
    let disk = root.join("target").join("test-disk.img");
    let _ = fs::remove_file(&disk);
    create_empty_disk(&disk)?;

    println!("test stage 1: installer");
    if !run_script(
        root,
        &Disks(vec![image.to_path_buf(), disk.clone()]),
        opts,
        STAGE1,
    )? {
        eprintln!("test: FAILED in stage 1");
        return Ok(ExitCode::FAILURE);
    }
    println!("test stage 2: installed system");
    if !run_script(root, &Disks(vec![disk]), opts, STAGE2)? {
        eprintln!("test: FAILED in stage 2");
        return Ok(ExitCode::FAILURE);
    }
    println!("test: PASSED");
    Ok(ExitCode::SUCCESS)
}

/// Boots headless, plays `script` over the serial console and checks the
/// responses. Returns `true` if every step passed and QEMU exited cleanly.
fn run_script(root: &Path, disks: &Disks, opts: &Options, script: &[Step]) -> Result<bool> {
    let timeout = Duration::from_secs(
        env::var("LLMOS_TEST_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(600),
    );
    let mut child = qemu_command(root, disks, opts)?
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
        for (input, expected) in script {
            if let Some(input) = input {
                println!("test: sending `{input}`");
                stdin.write_all(input.as_bytes())?;
                stdin.write_all(b"\r")?;
                stdin.flush()?;
            }
            wait_for(expected)?;
        }
        Ok(())
    })();

    let status = match result {
        Ok(()) => {
            // The last command powers off; give QEMU a moment to exit.
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
    Ok(matches!(status, Some(s) if s.code() == Some(QEMU_EXIT_SUCCESS)))
}
