pub(super) fn parse(input: &str) -> (String, Option<String>) {
    let mut parts = input.splitn(2, '|');
    let title = parts.next().unwrap_or_default().trim().to_string();
    let prompt = parts
        .next()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(str::to_string);
    (title, prompt)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_bare_title_without_prompt() {
        assert_eq!(parse("worker"), ("worker".to_string(), None));
    }

    #[test]
    fn parses_title_and_initial_prompt() {
        assert_eq!(
            parse("title | do the thing"),
            ("title".to_string(), Some("do the thing".to_string()))
        );
    }

    #[test]
    fn ignores_empty_initial_prompt() {
        assert_eq!(parse("title |"), ("title".to_string(), None));
        assert_eq!(parse("title |   "), ("title".to_string(), None));
    }

    #[test]
    fn parses_whitespace_only_input_as_empty_title_without_prompt() {
        assert_eq!(parse("   "), ("".to_string(), None));
    }

    #[test]
    fn trims_title_and_initial_prompt() {
        assert_eq!(
            parse("  title  |  do the thing  "),
            ("title".to_string(), Some("do the thing".to_string()))
        );
    }

    #[test]
    fn splits_only_on_the_first_delimiter() {
        assert_eq!(
            parse("title | a | b"),
            ("title".to_string(), Some("a | b".to_string()))
        );
    }
}
