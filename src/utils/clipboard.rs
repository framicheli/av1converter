use std::io::Write;
use std::process::{Command, Stdio};

/// Copy `text` to the system clipboard. Returns false when no clipboard
/// utility exists on this machine.
pub fn copy_to_clipboard(text: &str) -> bool {
    let commands: &[&[&str]] = match std::env::consts::OS {
        "macos" => &[&["pbcopy"]],
        "windows" => &[&["clip"]],
        _ => &[
            &["wl-copy"],
            &["xclip", "-selection", "clipboard"],
            &["xsel", "--clipboard", "--input"],
        ],
    };
    commands.iter().any(|argv| {
        let Ok(mut child) = Command::new(argv[0])
            .args(&argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let Some(mut stdin) = child.stdin.take() else {
            return false;
        };
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
        drop(stdin);
        child.wait().is_ok_and(|s| s.success())
    })
}
