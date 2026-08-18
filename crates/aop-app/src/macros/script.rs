//! Macro text, in both directions.
//!
//! What is stored is the script, not a serialised `Vec<Cmd>`. A planner who
//! opens a macro has to be able to read it, change a number, and have the
//! changed thing be what runs. The moment the file holds one representation and
//! the editor shows another, the two drift and the editor becomes a lie.
//!
//! The format is deliberately dull: `//` comments, one call per line, arguments
//! in brackets, a semicolon at the end. There are no expressions, no variables
//! and no control flow, because every one of those is a thing a reader would
//! have to work out rather than read.

use super::{Cmd, MacroError, FORMAT_VERSION};

/// What the comment header at the top of a script says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    pub name: Option<String>,
    pub description: Option<String>,
    pub format: Option<u32>,
}

/// Write a whole macro out as the text that would be stored on disk.
pub fn to_script(name: &str, description: &str, body: &[Cmd]) -> String {
    let mut out = String::new();
    out.push_str("// Alterion Open Project macro\n");
    out.push_str(&header_line("name:", name));
    out.push_str(&header_line("description:", &one_line(description)));
    out.push_str(&header_line("format:", &FORMAT_VERSION.to_string()));
    out.push('\n');
    for command in body {
        out.push_str(&command.to_script());
        out.push('\n');
    }
    out
}

/// Read the header back off a script.
///
/// Kept separate from parsing the body so that a standalone `.aopm` file can be
/// listed by name and description without running anything in it.
pub fn read_header(text: &str) -> Header {
    let mut header = Header::default();
    for line in text.lines() {
        let trimmed = line.trim();
        // The header is the comment block at the top. Anything else ends it,
        // so a `// name:` written halfway down is a comment and nothing more.
        if trimmed.is_empty() {
            continue;
        }
        let Some(comment) = trimmed.strip_prefix("//") else {
            break;
        };
        let comment = comment.trim();
        if let Some(value) = field_after(comment, "name:") {
            header.name = Some(value);
        } else if let Some(value) = field_after(comment, "description:") {
            header.description = Some(value);
        } else if let Some(value) = field_after(comment, "format:") {
            header.format = value.parse().ok();
        }
    }
    header
}

/// Read a script into the commands it asks for.
pub fn parse(text: &str) -> Result<Vec<Cmd>, MacroError> {
    Ok(parse_lines(text)?
        .into_iter()
        .map(|(_, command)| command)
        .collect())
}

/// Read a script, keeping the line each command came from.
///
/// Running a macro needs this: a command that fails halfway through has to be
/// able to say which line of the script the planner should go and look at.
pub fn parse_lines(text: &str) -> Result<Vec<(usize, Cmd)>, MacroError> {
    let mut out = Vec::new();
    for (offset, raw) in text.lines().enumerate() {
        let line = offset + 1;
        let code = strip_comment(raw).map_err(|why| MacroError::syntax(line, why))?;
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        let command = parse_statement(code).map_err(|why| MacroError::syntax(line, why))?;
        out.push((line, command));
    }
    Ok(out)
}

// ---- one line at a time -------------------------------------------------

/// Cut a line off at the comment, leaving `//` inside quoted text alone.
fn strip_comment(line: &str) -> Result<&str, String> {
    let bytes = line.as_bytes();
    let mut in_text = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if in_text => index += 1,
            b'"' => in_text = !in_text,
            b'/' if !in_text && bytes.get(index + 1) == Some(&b'/') => {
                return Ok(&line[..index]);
            }
            _ => {}
        }
        index += 1;
    }
    if in_text {
        return Err("the text on this line is missing its closing quote".to_string());
    }
    Ok(line)
}

/// Read one `name(args);` statement.
fn parse_statement(code: &str) -> Result<Cmd, String> {
    let Some(open) = code.find('(') else {
        return Err(format!(
            "expected a command such as indent(); this line says {code}"
        ));
    };
    let name = code[..open].trim();
    check_name(name)?;

    let close = closing_bracket(code, open)?;
    let inside = &code[open + 1..close];

    let after = code[close + 1..].trim();
    match after.strip_prefix(';') {
        None => Err(format!("{name}(...) needs a semicolon after it")),
        Some(rest) if !rest.trim().is_empty() => Err(format!(
            "there is more on this line after the semicolon: {}",
            rest.trim()
        )),
        Some(_) => Cmd::from_call(name, &split_arguments(inside)?),
    }
}

/// Commands are written the way the rest of the code names things.
fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("there is no command name before the bracket".to_string());
    }
    let ok = name
        .chars()
        .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_');
    if ok {
        Ok(())
    } else {
        Err(format!("{name} is not a command name"))
    }
}

/// Find the bracket that closes the one at `open`, ignoring quoted text.
fn closing_bracket(code: &str, open: usize) -> Result<usize, String> {
    let bytes = code.as_bytes();
    let mut in_text = false;
    let mut depth = 0usize;
    let mut index = open;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if in_text => index += 1,
            b'"' => in_text = !in_text,
            b'(' if !in_text => depth += 1,
            b')' if !in_text => {
                depth -= 1;
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    Err("the brackets on this line are not closed".to_string())
}

/// Split the inside of the brackets on commas, leaving quoted text alone.
fn split_arguments(inside: &str) -> Result<Vec<String>, String> {
    if inside.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    let mut characters = inside.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' if in_text => {
                current.push(character);
                if let Some(escaped) = characters.next() {
                    current.push(escaped);
                }
            }
            '"' => {
                in_text = !in_text;
                current.push(character);
            }
            ',' if !in_text => {
                out.push(std::mem::take(&mut current));
            }
            other => current.push(other),
        }
    }
    out.push(current);

    let trimmed: Vec<String> = out.iter().map(|piece| piece.trim().to_string()).collect();
    if trimmed.iter().any(String::is_empty) {
        return Err("there is an empty argument between the commas".to_string());
    }
    Ok(trimmed)
}

// ---- header helpers -----------------------------------------------------

/// The header labels line their values up, so the block reads as a table.
const LABEL_WIDTH: usize = 13;

fn header_line(label: &str, value: &str) -> String {
    format!("// {label:<LABEL_WIDTH$}{value}\n")
}

fn field_after(comment: &str, label: &str) -> Option<String> {
    comment
        .strip_prefix(label)
        .map(|value| value.trim().to_string())
}

/// A description is one line in the header, whatever it was when it was typed.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macros::cmd::Row;

    const RECORDED: &str = "\
// Alterion Open Project macro
// name:        Indent_And_Half_Done
// description: Indents tasks 3 to 7 and marks them half complete.
// format:      1

select_rows(3, 7);
indent();
set_percent_complete(50);
";

    fn indent_and_half_done() -> Vec<Cmd> {
        vec![
            Cmd::SelectRows {
                from: Row(3),
                to: Row(7),
            },
            Cmd::Indent {},
            Cmd::SetPercentComplete { percent: 50 },
        ]
    }

    #[test]
    fn a_recording_is_written_exactly_the_way_the_format_is_documented() {
        let written = to_script(
            "Indent_And_Half_Done",
            "Indents tasks 3 to 7 and marks them half complete.",
            &indent_and_half_done(),
        );
        assert_eq!(written, RECORDED);
    }

    #[test]
    fn the_written_script_is_the_script_that_runs() {
        let written = to_script("Whatever", "", &indent_and_half_done());
        assert_eq!(parse(&written).expect("it wrote it"), indent_and_half_done());
    }

    #[test]
    fn the_header_can_be_read_without_running_anything() {
        let header = read_header(RECORDED);
        assert_eq!(header.name.as_deref(), Some("Indent_And_Half_Done"));
        assert_eq!(
            header.description.as_deref(),
            Some("Indents tasks 3 to 7 and marks them half complete.")
        );
        assert_eq!(header.format, Some(FORMAT_VERSION));
    }

    #[test]
    fn a_header_written_further_down_is_only_a_comment() {
        let text = "indent();\n// name: Sneaky\n";
        assert_eq!(read_header(text), Header::default());
    }

    #[test]
    fn blank_lines_and_comments_are_not_commands() {
        let text = "// a note\n\n   \nindent();  // and another\n";
        assert_eq!(parse(text).expect("valid"), vec![Cmd::Indent {}]);
    }

    #[test]
    fn a_comment_marker_inside_quoted_text_is_part_of_the_text() {
        let text = r#"note("half // done");"#;
        assert_eq!(
            parse(text).expect("valid"),
            vec![Cmd::Note {
                message: "half // done".to_string()
            }]
        );
    }

    #[test]
    fn a_comma_inside_quoted_text_does_not_split_the_arguments() {
        let text = r#"append_task("Design, build, ship");"#;
        assert_eq!(
            parse(text).expect("valid"),
            vec![Cmd::AppendTask {
                name: "Design, build, ship".to_string()
            }]
        );
    }

    #[test]
    fn the_line_number_points_at_the_line_that_is_wrong() {
        let text = "indent();\noutdent();\nsummon_the_moon();\nindent();\n";
        let error = parse(text).expect_err("no such command");
        match error {
            MacroError::Syntax { line, ref message } => {
                assert_eq!(line, 3, "{error}");
                assert!(message.contains("summon_the_moon"), "{error}");
            }
            other => panic!("expected a syntax error, got {other}"),
        }
    }

    #[test]
    fn a_missing_semicolon_is_reported_on_its_own_line() {
        let error = parse("indent();\noutdent()\n").expect_err("no semicolon");
        assert!(matches!(error, MacroError::Syntax { line: 2, .. }), "{error}");
        assert!(error.to_string().contains("semicolon"), "{error}");
    }

    #[test]
    fn an_unclosed_bracket_is_reported() {
        let error = parse("select_rows(3, 7;\n").expect_err("no closing bracket");
        assert!(matches!(error, MacroError::Syntax { line: 1, .. }), "{error}");
        assert!(error.to_string().contains("brackets"), "{error}");
    }

    #[test]
    fn an_unclosed_quote_is_reported() {
        let error = parse("note(\"never ends);\n").expect_err("no closing quote");
        assert!(matches!(error, MacroError::Syntax { line: 1, .. }), "{error}");
        assert!(error.to_string().contains("quote"), "{error}");
    }

    #[test]
    fn a_line_with_no_call_on_it_says_so() {
        let error = parse("indent;\n").expect_err("not a call");
        assert!(matches!(error, MacroError::Syntax { line: 1, .. }), "{error}");
    }

    #[test]
    fn two_statements_on_one_line_are_refused_rather_than_half_read() {
        // Allowing this would mean an error could no longer name a line.
        let error = parse("indent(); outdent();\n").expect_err("two on a line");
        assert!(error.to_string().contains("after the semicolon"), "{error}");
    }

    #[test]
    fn an_argument_of_the_wrong_shape_names_the_argument() {
        let error = parse("select_row(third);\n").expect_err("not a number");
        assert!(error.to_string().contains("row"), "{error}");
    }

    #[test]
    fn an_empty_argument_between_commas_is_refused() {
        let error = parse("select_rows(3,,7);\n").expect_err("empty argument");
        assert!(error.to_string().contains("empty argument"), "{error}");
    }

    #[test]
    fn the_line_number_a_command_came_from_is_kept() {
        let text = "// header\n\nindent();\noutdent();\n";
        let lines: Vec<usize> = parse_lines(text)
            .expect("valid")
            .into_iter()
            .map(|(line, _)| line)
            .collect();
        assert_eq!(lines, vec![3, 4]);
    }

    #[test]
    fn a_description_over_several_lines_is_folded_into_the_header() {
        let written = to_script("Name", "first line\n   second line", &[]);
        assert!(
            written.contains("// description: first line second line\n"),
            "{written}"
        );
    }
}
