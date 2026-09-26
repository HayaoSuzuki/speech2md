use std::fmt::Write as _;

use yasumaro_core::{Timestamp, TranscriptDocument};

const COMMONMARK_ESCAPABLE: &str = r##"!"#$%&'()*+,-./:;<=>?@[\]^_`{|}~"##;

fn push_escaped(output: &mut String, character: char) {
    if COMMONMARK_ESCAPABLE.contains(character) {
        output.push('\\');
    }
    output.push(character);
}

fn escape_commonmark(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        push_escaped(&mut escaped, character);
    }
    escaped
}

fn escape_block_text(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut escaped = String::with_capacity(normalized.len());
    let mut at_line_start = true;
    for character in normalized.chars() {
        match character {
            ' ' if at_line_start => escaped.push_str("&#32;"),
            '\t' if at_line_start => escaped.push_str("&#9;"),
            '\n' => {
                escaped.push('\n');
                at_line_start = true;
            }
            _ => {
                push_escaped(&mut escaped, character);
                at_line_start = false;
            }
        }
    }
    escaped
}

fn escape_inline(text: &str) -> String {
    let single_line = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
    escape_commonmark(&single_line)
}

fn format_timestamp(timestamp: Timestamp) -> String {
    let total_seconds = timestamp.as_millis() / 1_000;
    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minutes = total_minutes % 60;
    let hours = total_minutes / 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[must_use]
pub fn render_commonmark(document: &TranscriptDocument) -> String {
    let mut output = format!("# {}\n", escape_inline(&document.title));

    for utterance in &document.utterances {
        let speaker = utterance.speaker.map_or_else(
            || "Unknown".to_owned(),
            |id| format!("Speaker {}", u64::from(id.as_u32()) + 1),
        );
        writeln!(
            output,
            "\n**{speaker}**（{}）\n\n{}",
            format_timestamp(utterance.span.start()),
            escape_block_text(&utterance.text)
        )
        .expect("writing to a String cannot fail");
    }

    output
}
