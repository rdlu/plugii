//! Running the few external commands the readers ask (`journalctl`, `niri`)
//! without ever failing or hanging the caller.

use std::env;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Whether `program` is an executable file in a `$PATH` directory.
pub fn on_path(program: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|dir| is_executable(&dir.join(program)))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// `program args…`'s stdout, or `None` when it can't start, exits non-zero,
/// or is still running after `timeout` (it is killed then).
pub fn output(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Read on a thread so a chatty child can't block on a full pipe while we
    // wait for it.
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).map(|_| buf)
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let buf = reader.join().ok()?.ok()?;
    status
        .filter(|s| s.success())
        .map(|_| String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_of_a_quick_command() {
        let out = output("sh", &["-c", "echo hi"], Duration::from_secs(5));
        assert_eq!(out.as_deref(), Some("hi\n"));
    }

    #[test]
    fn failure_and_timeout_give_none() {
        assert_eq!(
            output("sh", &["-c", "exit 3"], Duration::from_secs(5)),
            None
        );
        assert_eq!(
            output("plugii-no-such-program", &[], Duration::from_secs(1)),
            None
        );
        let started = Instant::now();
        assert_eq!(output("sleep", &["10"], Duration::from_millis(100)), None);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn on_path_finds_sh() {
        assert!(on_path("sh"));
        assert!(!on_path("plugii-no-such-program"));
    }
}
