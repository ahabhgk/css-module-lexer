#![allow(dead_code)]

use std::borrow::Cow;

const REPLACEMENT_CHARACTER: char = '\u{FFFD}';

#[derive(Clone, Copy)]
pub(crate) struct Escape {
    pub end: usize,
    pub value: Option<char>,
}

/// Decodes CSS escape sequences in `input`.
///
/// Returns the original slice when it contains no escapes or null characters.
///
/// ```
/// use rocketcss_parser::unescape;
///
/// assert_eq!(unescape(r"fo\6f "), "foo");
/// ```
pub fn unescape(input: &str) -> Cow<'_, str> {
    let Some(first_escape) = input.bytes().position(|byte| matches!(byte, b'\\' | b'\0')) else {
        return Cow::Borrowed(input);
    };

    let mut output = String::with_capacity(input.len());
    output.push_str(&input[..first_escape]);

    let mut position = first_escape;
    while position < input.len() {
        match input.as_bytes()[position] {
            b'\\' => {
                let escape = parse_escape(input, position);
                if let Some(value) = escape.value {
                    output.push(value);
                }
                position = escape.end;
            }
            b'\0' => {
                output.push(REPLACEMENT_CHARACTER);
                position += 1;
            }
            _ => {
                let value = input[position..].chars().next().unwrap();
                output.push(value);
                position += value.len_utf8();
            }
        }
    }

    Cow::Owned(output)
}

pub(crate) fn eq_ignore_ascii_case(input: &str, expected: &str) -> bool {
    let mut expected = expected.chars();
    let mut position = 0;

    while position < input.len() {
        let (value, end) = match input.as_bytes()[position] {
            b'\\' => {
                let escape = parse_escape(input, position);
                (escape.value, escape.end)
            }
            b'\0' => (Some(REPLACEMENT_CHARACTER), position + 1),
            _ => {
                let value = input[position..].chars().next().unwrap();
                (Some(value), position + value.len_utf8())
            }
        };
        position = end;

        let Some(value) = value else {
            continue;
        };
        let Some(expected) = expected.next() else {
            return false;
        };
        if !value.eq_ignore_ascii_case(&expected) {
            return false;
        }
    }

    expected.next().is_none()
}

/// Returns the byte length of an encoded prefix after decoding CSS escapes.
pub(crate) fn raw_prefix_len(input: &str, expected: &str) -> Option<usize> {
    let mut position = 0;

    for expected in expected.chars() {
        if position >= input.len() {
            return None;
        }
        let (value, end) = match input.as_bytes()[position] {
            b'\\' => {
                let escape = parse_escape(input, position);
                (escape.value, escape.end)
            }
            b'\0' => (Some(REPLACEMENT_CHARACTER), position + 1),
            _ => {
                let value = input[position..].chars().next().unwrap();
                (Some(value), position + value.len_utf8())
            }
        };
        position = end;
        if value != Some(expected) {
            return None;
        }
    }

    Some(position)
}

pub(crate) fn parse_escape(input: &str, start: usize) -> Escape {
    debug_assert_eq!(input.as_bytes()[start], b'\\');

    let bytes = input.as_bytes();
    let mut position = start + 1;
    if position == bytes.len() {
        return Escape {
            end: position,
            value: Some(REPLACEMENT_CHARACTER),
        };
    }

    if matches!(bytes[position], b'\n' | b'\r' | b'\x0C') {
        if bytes[position] == b'\r' && bytes.get(position + 1) == Some(&b'\n') {
            position += 1;
        }
        return Escape {
            end: position + 1,
            value: None,
        };
    }

    if bytes[position].is_ascii_hexdigit() {
        let mut value = 0;
        let mut digits = 0;
        while position < bytes.len() && digits < 6 {
            let Some(digit) = hex_digit(bytes[position]) else {
                break;
            };
            value = value * 16 + digit;
            position += 1;
            digits += 1;
        }

        if position < bytes.len() {
            match bytes[position] {
                b' ' | b'\t' => position += 1,
                b'\n' | b'\x0C' => position += 1,
                b'\r' => {
                    position += 1;
                    if bytes.get(position) == Some(&b'\n') {
                        position += 1;
                    }
                }
                _ => {}
            }
        }

        return Escape {
            end: position,
            value: Some(
                char::from_u32(value)
                    .filter(|_| value != 0)
                    .unwrap_or(REPLACEMENT_CHARACTER),
            ),
        };
    }

    if bytes[position] == b'\0' {
        return Escape {
            end: position + 1,
            value: Some(REPLACEMENT_CHARACTER),
        };
    }

    let value = input[position..].chars().next().unwrap();
    Escape {
        end: position + value.len_utf8(),
        value: Some(value),
    }
}

fn hex_digit(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some((byte - b'0') as u32),
        b'a'..=b'f' => Some((byte - b'a' + 10) as u32),
        b'A'..=b'F' => Some((byte - b'A' + 10) as u32),
        _ => None,
    }
}
