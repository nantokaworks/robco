mod glyphs_a;
mod glyphs_b;
mod glyphs_c;

const HEIGHT: usize = 6;

pub(super) fn render_fitting(input: &str, width: usize) -> Option<Vec<String>> {
    let chars: Vec<char> = input.to_uppercase().chars().collect();
    let glyphs: Option<Vec<_>> = chars.into_iter().map(glyph).collect();
    let glyphs = glyphs?;
    if glyphs.is_empty() || rendered_width(&glyphs) > width {
        return None;
    }
    let mut rows = vec![String::new(); HEIGHT];
    for (index, glyph) in glyphs.into_iter().enumerate() {
        let glyph_width = glyph_width(&glyph);
        for (row, part) in rows.iter_mut().zip(glyph) {
            if index > 0 {
                row.push(' ');
            }
            row.push_str(part);
            row.extend(std::iter::repeat_n(
                ' ',
                glyph_width.saturating_sub(part.chars().count()),
            ));
        }
    }
    Some(rows)
}

fn rendered_width(glyphs: &[[&str; HEIGHT]]) -> usize {
    glyphs.iter().map(glyph_width).sum::<usize>() + glyphs.len().saturating_sub(1)
}

fn glyph_width(glyph: &[&str; HEIGHT]) -> usize {
    glyph
        .iter()
        .map(|row| row.chars().count())
        .max()
        .unwrap_or(0)
}

fn glyph(character: char) -> Option<[&'static str; HEIGHT]> {
    glyphs_a::glyph(character)
        .or_else(|| glyphs_b::glyph(character))
        .or_else(|| glyphs_c::glyph(character))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_six_equal_width_rows() {
        let rows = render_fitting("ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789-_.", 500).unwrap();
        assert_eq!(rows.len(), HEIGHT);
        let width = rows[0].chars().count();
        assert!(rows.iter().all(|row| row.chars().count() == width));
    }

    #[test]
    fn unsupported_character_falls_back() {
        assert!(render_fitting("café", 80).is_none());
    }

    #[test]
    fn narrow_width_falls_back() {
        assert!(render_fitting("AB", 16).is_none());
        assert!(render_fitting("AB", 17).is_some());
    }
}
