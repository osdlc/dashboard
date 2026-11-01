//! Text from outside the tool (a file's keys, a path, a plugin's message)
//! made safe to print on a terminal. Port of `stripControls` in `@osdlc/ux`.

const ESC: char = '\u{1b}';
const BEL: char = '\u{07}';
/// 8-bit CSI and string terminator.
const C1_CSI: char = '\u{9b}';
const C1_ST: char = '\u{9c}';

/// `text` with every terminal control removed except line feeds and tabs, so
/// printing it cannot move the cursor, recolor the terminal, set its title or
/// write the clipboard.
///
/// Removed: escape sequences, whole (CSI; OSC, DCS, SOS, PM and APC strings
/// up to BEL, ST or the next ESC; two-character escapes), their 8-bit C1
/// forms and every other C1 control, C0 controls except `\n` and `\t` (a
/// carriage return is always removed), DEL, and the bidirectional controls.
pub fn safe(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let at = |i: usize| chars.get(i).copied();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    // The index after a string body that starts at `i`: past BEL or ST, or at
    // an ESC that is not ST.
    let skip_string = |mut i: usize| {
        while i < n {
            match chars[i] {
                BEL | C1_ST => return i + 1,
                ESC => return if at(i + 1) == Some('\\') { i + 2 } else { i },
                _ => i += 1,
            }
        }
        n
    };
    // The index after CSI parameters and intermediates, past the final byte.
    let skip_csi = |mut i: usize| {
        while i < n {
            match chars[i] {
                '\u{40}'..='\u{7e}' => return i + 1,
                '\u{20}'..='\u{3f}' => i += 1,
                // A character that cannot be in a CSI ends it.
                _ => return i,
            }
        }
        n
    };

    while i < n {
        let c = chars[i];
        match c {
            ESC => {
                i = match at(i + 1) {
                    Some('[') => skip_csi(i + 2),
                    Some(']' | 'P' | 'X' | '^' | '_') => skip_string(i + 2),
                    Some('\u{20}'..='\u{7e}') => {
                        // Intermediates 0x20-0x2F, then one final byte.
                        let mut j = i + 1;
                        while matches!(at(j), Some('\u{20}'..='\u{2f}')) {
                            j += 1;
                        }
                        if matches!(at(j), Some('\u{30}'..='\u{7e}')) {
                            j + 1
                        } else {
                            j
                        }
                    }
                    _ => i + 1,
                };
            }
            C1_CSI => i = skip_csi(i + 1),
            '\u{9d}' | '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => i = skip_string(i + 1),
            '\n' | '\t' => {
                out.push(c);
                i += 1;
            }
            _ if is_control(c) => i += 1,
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// C0, DEL, C1 and the bidirectional controls.
fn is_control(c: char) -> bool {
    matches!(c,
        '\u{0}'..='\u{1f}'
        | '\u{7f}'..='\u{9f}'
        | '\u{61c}'
        | '\u{200e}'
        | '\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_escapes_and_controls() {
        assert_eq!(safe("plain text ✓"), "plain text ✓");
        assert_eq!(safe("a\x1b[31mred\x1b[0m b"), "ared b");
        assert_eq!(safe("x\x1b]0;title\x07y"), "xy");
        assert_eq!(safe("x\x1b]52;c;Zm9v\x1b\\y"), "xy");
        assert_eq!(safe("one\r\ntwo\tthree\x08"), "one\ntwo\tthree");
        assert_eq!(safe("a\u{9b}2Jb"), "ab");
        assert_eq!(safe("a\u{202e}b\u{7f}"), "ab");
        assert_eq!(safe("a\x1bcb"), "ab");
        assert_eq!(safe("tail\x1b"), "tail");
    }
}
