#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Status,
    Dispatch(bool),
    AutoMerge(bool),
    Workers,
    Tasks,
    Skip(String),
    Retry(String),
    TaskCreate {
        repo: String,
        title: String,
        description: Option<String>,
    },
    Answer {
        agent: String,
        text: String,
    },
    Approve(String),
    Kill(String),
    Log(usize),
    Panic,
    Merge(String),
    Diff(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Command(Command),
    Confirm(String),
}

pub fn parse(message: &str) -> Option<Input> {
    let message = message.trim();
    if let Some(code) = message.strip_prefix("CONFIRM ") {
        let code = code.trim();
        return (!code.is_empty()).then(|| Input::Confirm(code.to_string()));
    }
    let body = message.strip_prefix('!')?;
    let mut parts = body.split_whitespace();
    let name = parts.next()?.to_ascii_lowercase();
    let command = match name.as_str() {
        "status" if parts.next().is_none() => Command::Status,
        "workers" if parts.next().is_none() => Command::Workers,
        "tasks" if parts.next().is_none() => Command::Tasks,
        "panic" if parts.next().is_none() => Command::Panic,
        "dispatch" => Command::Dispatch(on_off(parts.next()?, parts.next())?),
        "automerge" => Command::AutoMerge(on_off(parts.next()?, parts.next())?),
        "skip" => Command::Skip(single(&mut parts)?),
        "retry" => Command::Retry(single(&mut parts)?),
        "approve" => Command::Approve(single(&mut parts)?),
        "kill" => Command::Kill(single(&mut parts)?),
        "merge" => Command::Merge(single(&mut parts)?),
        "diff" => Command::Diff(single(&mut parts)?),
        "log" => {
            let limit = match parts.next() {
                Some(value) => value.parse().ok()?,
                None => 10,
            };
            if parts.next().is_some() {
                return None;
            }
            Command::Log(limit.min(50))
        }
        "answer" => {
            let rest = body[name.len()..].trim();
            let (agent, text) = rest.split_once(char::is_whitespace)?;
            let text = text.trim();
            if agent.is_empty() || text.is_empty() {
                return None;
            }
            Command::Answer {
                agent: agent.into(),
                text: text.into(),
            }
        }
        _ => return None,
    };
    Some(Input::Command(command))
}

fn on_off(value: &str, extra: Option<&str>) -> Option<bool> {
    if extra.is_some() {
        return None;
    }
    match value.to_ascii_lowercase().as_str() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

fn single<'a>(parts: &mut impl Iterator<Item = &'a str>) -> Option<String> {
    let value = parts.next()?.to_string();
    parts.next().is_none().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands_without_gateway_types() {
        assert_eq!(
            parse("!dispatch off"),
            Some(Input::Command(Command::Dispatch(false)))
        );
        assert_eq!(
            parse("!answer worker-1 yes, proceed"),
            Some(Input::Command(Command::Answer {
                agent: "worker-1".into(),
                text: "yes, proceed".into(),
            }))
        );
        assert_eq!(
            parse("CONFIRM 123456"),
            Some(Input::Confirm("123456".into()))
        );
        assert_eq!(parse("!kill a extra"), None);
        assert_eq!(
            parse("!merge #448"),
            Some(Input::Command(Command::Merge("#448".into())))
        );
        assert_eq!(
            parse("!diff #448"),
            Some(Input::Command(Command::Diff("#448".into())))
        );
        assert_eq!(parse("!merge"), None);
    }
}
