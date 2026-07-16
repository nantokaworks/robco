use std::io::{BufRead, Write};

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
