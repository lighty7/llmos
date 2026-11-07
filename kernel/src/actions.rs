//! System actions: the single, validated path through which both typed shell
//! commands and LLM decisions change the system.
//!
//! The LLM never executes anything directly. It produces JSON such as
//! `{"action": "write_file", "path": "notes.txt", "content": "hi"}`, which
//! [`Action::from_json`] parses and validates into an [`Action`]. Destructive
//! actions report [`Action::needs_confirmation`] so the shell can ask the user
//! before [`Action::execute`] runs them.
//!
//! The list of actions offered to the model lives in
//! [`llm::agent::SHELL_ACTIONS`]; the `selftest` command verifies that every
//! entry there parses here.

use alloc::string::{String, ToString};

use serde::Deserialize;

use crate::framebuffer::Color;
use crate::{console, fs, power, print, println, storage};

/// Something the system can do on behalf of the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// List all files.
    ListFiles,
    /// Print a file.
    ReadFile {
        /// File name.
        path: String,
    },
    /// Create or overwrite a file.
    WriteFile {
        /// File name.
        path: String,
        /// New contents.
        content: String,
    },
    /// Delete a file.
    DeleteFile {
        /// File name.
        path: String,
    },
    /// Print the hardware inventory summary.
    ShowHardware,
    /// Print memory usage.
    ShowMemory,
    /// Clear the screen.
    ClearScreen,
    /// Show the help text.
    Help,
    /// Reply to the user with text (no system change).
    Say {
        /// The reply.
        text: String,
    },
    /// Power the machine off.
    Poweroff,
    /// Reboot the machine.
    Reboot,
    /// List the disks and their partitions.
    ListDisks,
    /// Install llmos onto a disk (the only empty one if `disk` is `None`).
    Install {
        /// Target disk name, e.g. `vd1`.
        disk: Option<String>,
    },
}

/// Raw JSON shape produced by the LLM, before validation.
#[derive(Debug, Deserialize)]
struct RawAction {
    action: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

/// Why an action could not be accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    /// The text was not valid JSON of the expected shape.
    Malformed(String),
    /// The action name is unknown.
    UnknownAction(String),
    /// A required field is missing.
    MissingField(&'static str),
    /// The file name is invalid.
    InvalidPath(String),
}

impl core::fmt::Display for ActionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ActionError::Malformed(e) => write!(f, "malformed action JSON: {e}"),
            ActionError::UnknownAction(a) => write!(f, "unknown action `{a}`"),
            ActionError::MissingField(field) => write!(f, "missing field `{field}`"),
            ActionError::InvalidPath(p) => write!(f, "invalid file name `{p}`"),
        }
    }
}

impl Action {
    /// Parses and validates an action from LLM-produced JSON.
    ///
    /// # Errors
    ///
    /// Returns an [`ActionError`] for malformed JSON, unknown action names,
    /// missing fields or invalid file names.
    pub fn from_json(json: &str) -> Result<Self, ActionError> {
        let raw: RawAction =
            serde_json::from_str(json).map_err(|e| ActionError::Malformed(e.to_string()))?;
        let path = || -> Result<String, ActionError> {
            let p = raw.path.clone().ok_or(ActionError::MissingField("path"))?;
            fs::validate_name(&p).map_err(|_| ActionError::InvalidPath(p.clone()))?;
            Ok(p)
        };
        Ok(match raw.action.as_str() {
            "list_files" => Action::ListFiles,
            "read_file" => Action::ReadFile { path: path()? },
            "write_file" => Action::WriteFile {
                path: path()?,
                content: raw
                    .content
                    .clone()
                    .ok_or(ActionError::MissingField("content"))?,
            },
            "delete_file" => Action::DeleteFile { path: path()? },
            "show_hardware" => Action::ShowHardware,
            "show_memory" => Action::ShowMemory,
            "clear_screen" => Action::ClearScreen,
            "help" => Action::Help,
            "say" => Action::Say {
                text: raw.text.clone().ok_or(ActionError::MissingField("text"))?,
            },
            "poweroff" => Action::Poweroff,
            "reboot" => Action::Reboot,
            "list_disks" => Action::ListDisks,
            "install" => Action::Install { disk: None },
            other => return Err(ActionError::UnknownAction(other.into())),
        })
    }

    /// Whether the user must confirm this action when it was proposed by the
    /// LLM: anything that destroys data or stops the machine.
    pub fn needs_confirmation(&self) -> bool {
        match self {
            Action::WriteFile { path, .. } => fs::exists(path),
            Action::DeleteFile { .. }
            | Action::Poweroff
            | Action::Reboot
            | Action::Install { .. } => true,
            _ => false,
        }
    }

    /// One-line description shown before running or confirming the action.
    pub fn describe(&self) -> String {
        use alloc::format;
        match self {
            Action::ListFiles => "list files".into(),
            Action::ReadFile { path } => format!("show the contents of {path}"),
            Action::WriteFile { path, content } => {
                format!("write {} bytes to {path}", content.len())
            }
            Action::DeleteFile { path } => format!("delete {path}"),
            Action::ShowHardware => "show the hardware inventory".into(),
            Action::ShowMemory => "show memory usage".into(),
            Action::ClearScreen => "clear the screen".into(),
            Action::Help => "show help".into(),
            Action::Say { .. } => "reply".into(),
            Action::Poweroff => "power off the machine".into(),
            Action::Reboot => "reboot the machine".into(),
            Action::ListDisks => "list disks".into(),
            Action::Install { disk } => match storage::install::plan(disk.as_deref()) {
                Ok(plan) => plan.describe(),
                Err(e) => format!("install llmos (not possible: {e})"),
            },
        }
    }

    /// Runs the action, printing its result.
    pub fn execute(&self, ctx: &crate::shell::Context) {
        match self {
            Action::ListFiles => {
                let files = fs::list();
                if files.is_empty() {
                    println!("(no files)");
                }
                for (name, size) in files {
                    println!("  {name:<32} {size:>8} bytes");
                }
            }
            Action::ReadFile { path } => match fs::read(path) {
                Ok(data) => println!("{}", String::from_utf8_lossy(&data)),
                Err(e) => error(&e),
            },
            Action::WriteFile { path, content } => match fs::write(path, content.as_bytes()) {
                Ok(()) => {
                    println!("wrote {} bytes to {path}", content.len());
                    persist();
                }
                Err(e) => error(&e),
            },
            Action::DeleteFile { path } => match fs::remove(path) {
                Ok(()) => {
                    println!("removed {path}");
                    persist();
                }
                Err(e) => error(&e),
            },
            Action::ShowHardware => ctx.inventory.print_summary(),
            Action::ShowMemory => {
                let (used, free) = crate::allocator::usage();
                println!("heap: {} KiB used, {} KiB free", used / 1024, free / 1024);
                println!(
                    "usable RAM at boot: {} MiB",
                    ctx.inventory.memory.usable_mib
                );
            }
            Action::ClearScreen => console::clear(),
            Action::Help => crate::shell::print_help(),
            Action::Say { text } => {
                console::set_color(Color::YELLOW);
                println!("{text}");
                console::set_color(Color::WHITE);
            }
            Action::Poweroff => {
                println!("powering off");
                power::poweroff();
            }
            Action::Reboot => {
                println!("rebooting");
                power::reboot();
            }
            Action::ListDisks => {
                let disks = storage::disk_info();
                if disks.is_empty() {
                    println!("(no supported disks)");
                }
                for d in disks {
                    println!(
                        "  {:<5} {:>7} MiB  {:<16} {}",
                        d.name,
                        d.size_mib,
                        d.role,
                        d.partitions.join(", ")
                    );
                }
                if let Some(name) = storage::mounted_disk() {
                    println!("files are stored on {name}");
                }
            }
            Action::Install { disk } => match storage::install::plan(disk.as_deref()) {
                Ok(plan) => match plan.execute() {
                    Ok(()) => println!(
                        "llmos is installed on {}. Files are now saved there; remove the boot media and reboot to start from it.",
                        plan.target_name()
                    ),
                    Err(e) => error(&e),
                },
                Err(e) => error(&e),
            },
        }
    }
}

/// Saves the files to the data partition, if one is mounted.
fn persist() {
    if let Err(e) = storage::sync() {
        error(&alloc::format!("could not save to disk: {e}"));
    }
}

fn error(e: &dyn core::fmt::Display) {
    console::set_color(Color::RED);
    print!("error: ");
    console::set_color(Color::WHITE);
    println!("{e}");
}

/// Splits a command line into the first word and the rest.
pub fn split_first_word(line: &str) -> (&str, &str) {
    let line = line.trim();
    match line.find(' ') {
        Some(i) => (&line[..i], line[i + 1..].trim_start()),
        None => (line, ""),
    }
}

/// Parses a typed shell command into an [`Action`], if it is one.
pub fn from_command(line: &str) -> Option<Result<Action, String>> {
    let (cmd, rest) = split_first_word(line);
    let need_path = |rest: &str| -> Result<String, String> {
        let p = rest.split_whitespace().next().ok_or("missing file name")?;
        fs::validate_name(p).map_err(|e| e.to_string())?;
        Ok(p.into())
    };
    Some(match cmd {
        "ls" => Ok(Action::ListFiles),
        "cat" => need_path(rest).map(|path| Action::ReadFile { path }),
        "write" => {
            let (name, content) = split_first_word(rest);
            need_path(name).map(|path| Action::WriteFile {
                path,
                content: content.into(),
            })
        }
        "rm" => need_path(rest).map(|path| Action::DeleteFile { path }),
        "info" => Ok(Action::ShowHardware),
        "mem" => Ok(Action::ShowMemory),
        "clear" => Ok(Action::ClearScreen),
        "help" => Ok(Action::Help),
        "poweroff" | "shutdown" | "exit" => Ok(Action::Poweroff),
        "reboot" => Ok(Action::Reboot),
        "disks" => Ok(Action::ListDisks),
        "install" => Ok(Action::Install {
            disk: rest.split_whitespace().next().map(String::from),
        }),
        _ => return None,
    })
}
