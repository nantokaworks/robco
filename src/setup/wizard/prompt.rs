use std::io::{BufRead, IsTerminal, Write};

use crate::{Error, Result};

fn read<R: BufRead, W: Write>(input: &mut R, output: &mut W, prompt: &str) -> Result<String> {
    write!(output, "▌ robco ▸ {prompt}")?;
    output.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Err(Error::Wizard(
            "input ended while waiting for an answer".into(),
        ));
    }
    Ok(line.trim().to_string())
}

pub(crate) fn text<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    label: &str,
    default: &str,
) -> Result<String> {
    let answer = read(input, output, &format!("{label} [{default}]: "))?;
    Ok(if answer.is_empty() {
        default.to_string()
    } else {
        answer
    })
}

pub(crate) fn validated_text<R, W, F>(
    input: &mut R,
    output: &mut W,
    label: &str,
    default: &str,
    invalid: &str,
    valid: F,
) -> Result<String>
where
    R: BufRead,
    W: Write,
    F: Fn(&str) -> bool,
{
    loop {
        let answer = text(input, output, label, default)?;
        if valid(&answer) {
            return Ok(answer);
        }
        writeln!(output, "▌ robco ▸ NG ··············· {invalid}")?;
    }
}

/// Prompt for a value that must not be echoed to the terminal — a secret
/// typed directly into the wizard rather than exported beforehand. Reads
/// through the same generic `BufRead` as every other prompt; the no-echo
/// toggle only touches the process's real stdin and only when it is a TTY,
/// so a `Cursor`-backed test harness degrades to a normal, echoed read.
pub(crate) fn secret_text<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    label: &str,
) -> Result<String> {
    write!(output, "▌ robco ▸ {label}: ")?;
    output.flush()?;
    let echo_guard = EchoGuard::disable();
    let mut line = String::new();
    let read = input.read_line(&mut line);
    drop(echo_guard);
    writeln!(output)?;
    if read? == 0 {
        return Err(Error::Wizard(
            "input ended while waiting for an answer".into(),
        ));
    }
    Ok(line.trim().to_string())
}

#[cfg(unix)]
struct EchoGuard {
    original: Option<libc::termios>,
}

#[cfg(unix)]
impl EchoGuard {
    fn disable() -> Self {
        if !std::io::stdin().is_terminal() {
            return Self { original: None };
        }
        // SAFETY: `STDIN_FILENO` names the process's own stdin, and `term` is
        // fully initialised by `tcgetattr` before any field of it is read.
        let original = unsafe {
            let mut term: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut term) != 0 {
                return Self { original: None };
            }
            term
        };
        let mut disabled = original;
        disabled.c_lflag &= !(libc::ECHO | libc::ECHONL);
        // SAFETY: `disabled` was derived from a successful `tcgetattr` above.
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &disabled);
        }
        Self {
            original: Some(original),
        }
    }
}

#[cfg(unix)]
impl Drop for EchoGuard {
    fn drop(&mut self) {
        if let Some(original) = self.original {
            // SAFETY: `original` was captured by a successful `tcgetattr` in `disable`.
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
            }
        }
    }
}

#[cfg(not(unix))]
struct EchoGuard;

#[cfg(not(unix))]
impl EchoGuard {
    fn disable() -> Self {
        Self
    }
}

pub(crate) fn confirm<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    label: &str,
    default: bool,
) -> Result<bool> {
    let hint = if default { "Y/n" } else { "y/N" };
    loop {
        match read(input, output, &format!("{label} [{hint}]: "))?
            .to_ascii_lowercase()
            .as_str()
        {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => writeln!(output, "▌ robco ▸ NG ··············· enter y or n")?,
        }
    }
}

pub(crate) fn select<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    label: &str,
    choices: &[String],
    default: usize,
) -> Result<usize> {
    if choices.is_empty() || default >= choices.len() {
        return Err(Error::Wizard("invalid select prompt configuration".into()));
    }
    writeln!(output, "▌ robco ▸ {label}")?;
    for (index, choice) in choices.iter().enumerate() {
        writeln!(output, "  {}. {choice}", index + 1)?;
    }
    number(input, output, "selection", default + 1, 1, choices.len()).map(|value| value - 1)
}

pub(crate) fn number<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    label: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize> {
    loop {
        let answer = read(input, output, &format!("{label} [{default}]: "))?;
        if answer.is_empty() {
            return Ok(default);
        }
        if let Ok(value) = answer.parse::<usize>()
            && (min..=max).contains(&value)
        {
            return Ok(value);
        }
        writeln!(output, "▌ robco ▸ NG ··············· enter {min}–{max}")?;
    }
}
