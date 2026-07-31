//! Splits one logical reply into Discord-sized messages at line boundaries,
//! closing and reopening ``` code fences across each split so every message
//! renders as well-formed markdown on its own.

/// Headroom under Discord's 2000-char message cap; the same budget the old
/// mid-line hard cut in `send_text` used.
pub(super) const MESSAGE_LIMIT: usize = 1900;

const FENCE: &str = "```";
/// Chars a flush appends while a fence is open: `"\n```"`.
const CLOSE_COST: usize = 4;

/// Splits `text` into messages of at most `limit` chars each, cutting at
/// line boundaries. A cut inside a code fence closes the fence at the end
/// of the message and reopens it (with its info string) at the start of
/// the next one. A single line longer than `limit` is the one case that
/// still hard-cuts mid-line.
pub(super) fn split_message(text: &str, limit: usize) -> Vec<String> {
    let mut chunks = Chunks::default();
    for line in text.split('\n') {
        chunks.push_line(line, limit);
    }
    chunks.finish()
}

#[derive(Default)]
struct Chunks {
    out: Vec<String>,
    lines: Vec<String>,
    /// Char count of `lines` joined with `\n`.
    len: usize,
    /// The reopen line (fence plus info string) while inside a code fence.
    fence: Option<String>,
}

impl Chunks {
    fn push_line(&mut self, line: &str, limit: usize) {
        let fence_after = self.fence_after(line);
        let close = if fence_after.is_some() { CLOSE_COST } else { 0 };
        let line_len = line.chars().count();
        // Flushing helps only when the line fits a fresh message (which
        // starts with the reopen line when a fence is open); a line too
        // long even for that fills the current message first instead.
        let fresh = self
            .fence
            .as_ref()
            .map_or(0, |reopen| reopen.chars().count() + 1);
        if !self.lines.is_empty()
            && self.len + 1 + line_len + close > limit
            && fresh + line_len + close <= limit
        {
            self.flush();
        }
        let sep = usize::from(!self.lines.is_empty());
        if self.len + sep + line_len + close > limit {
            self.push_oversized(line, limit, close);
        } else {
            self.len += sep + line_len;
            self.lines.push(line.to_string());
        }
        self.fence = fence_after;
    }

    /// Discord toggles code-fence state on every ``` occurrence, so a line
    /// carrying an odd number of them flips the state; the text after the
    /// last occurrence is the info string a reopened fence must repeat.
    fn fence_after(&self, line: &str) -> Option<String> {
        if line.matches(FENCE).count().is_multiple_of(2) {
            return self.fence.clone();
        }
        match self.fence {
            Some(_) => None,
            None => {
                let info = line.rsplit(FENCE).next().unwrap_or("").trim();
                Some(format!("{FENCE}{info}"))
            }
        }
    }

    /// Hard-cuts a line that cannot fit in one message even on its own.
    fn push_oversized(&mut self, line: &str, limit: usize, close: usize) {
        let mut rest: Vec<char> = line.chars().collect();
        loop {
            let sep = usize::from(!self.lines.is_empty());
            let room = limit.saturating_sub(self.len + sep + close).max(1);
            if rest.len() <= room {
                self.len += sep + rest.len();
                self.lines.push(rest.into_iter().collect());
                return;
            }
            let piece: String = rest.drain(..room).collect();
            self.len += sep + room;
            self.lines.push(piece);
            self.flush();
        }
    }

    fn flush(&mut self) {
        let mut chunk = self.lines.join("\n");
        if self.fence.is_some() {
            chunk.push_str("\n```");
        }
        self.out.push(chunk);
        self.lines.clear();
        self.len = 0;
        if let Some(reopen) = &self.fence {
            self.len = reopen.chars().count();
            self.lines.push(reopen.clone());
        }
    }

    /// A trailing unclosed fence is closed by the final flush too, so the
    /// last message renders as well-formed markdown like the others.
    fn finish(mut self) -> Vec<String> {
        self.flush();
        self.out
    }
}

#[cfg(test)]
#[path = "message_split_tests.rs"]
mod tests;
