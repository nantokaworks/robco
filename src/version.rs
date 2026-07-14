pub fn render() -> String {
    format!(
        "▌ robco ▸ version\n  █▀▄ █▀█ █▀▄ █▀▀ █▀█\n  █▀▄ █▄█ █▄▀ █▄▄ █▄█\n  version ················· {}\n",
        env!("CARGO_PKG_VERSION")
    )
}

pub fn print() {
    print!("{}", render());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_contains_crate_version() {
        assert!(render().contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn render_contains_non_empty_logo_lines() {
        let output = render();
        let mut lines = output.lines();
        assert_eq!(lines.next(), Some("▌ robco ▸ version"));
        assert!(lines.next().is_some_and(|line| !line.trim().is_empty()));
        assert!(lines.next().is_some_and(|line| !line.trim().is_empty()));
    }
}
