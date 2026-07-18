pub(super) fn parse(input: &str, with_prompt: bool) -> (String, Option<String>) {
    if !with_prompt {
        return (input.trim().to_string(), None);
    }

    let mut parts = input.splitn(2, '|');
    let title = parts.next().unwrap_or_default().trim().to_string();
    let prompt = parts
        .next()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(str::to_string);
    (title, prompt)
}
