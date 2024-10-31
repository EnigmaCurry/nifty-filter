pub fn reduce_blank_lines(input: &str) -> String {
    let mut result = String::new();
    let mut previous_blank = false;

    for line in input.lines() {
        if line.trim().is_empty() {
            if !previous_blank {
                result.push('\n');
            }
            previous_blank = true;
        } else {
            if line.trim().starts_with('#') && !result.ends_with("\n\n") {
                // Ensure a blank line before a comment
                result.push('\n');
            }
            if !result.is_empty() && !previous_blank {
                result.push('\n');
            }
            result.push_str(line);
            previous_blank = false;
        }
    }

    // Remove blank lines directly after opening braces
    let mut final_result = String::new();
    let mut lines = result.lines().peekable();
    while let Some(line) = lines.next() {
        final_result.push_str(line);
        final_result.push('\n');
        if line.trim().ends_with('{') {
            while let Some(next_line) = lines.peek() {
                if next_line.trim().is_empty() {
                    lines.next();
                } else {
                    break;
                }
            }
        }
    }

    // Remove blank lines between consecutive comments without removing any comments
    let mut cleaned_result = String::new();
    let mut lines = final_result.lines().peekable();
    while let Some(line) = lines.next() {
        cleaned_result.push_str(line);
        cleaned_result.push('\n');
        if line.trim().starts_with('#') {
            // Look ahead to find blank lines and skip them if followed by another comment
            while let Some(next_line) = lines.peek().cloned() {
                if next_line.trim().is_empty() {
                    lines.next();
                } else if next_line.trim().starts_with('#') {
                    cleaned_result.push_str(lines.next().unwrap());
                    cleaned_result.push('\n');
                } else {
                    break;
                }
            }
        }
    }

    cleaned_result.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reduce_blank_lines() {
        let input = "line 1\n\n\nline 2\n\n# Comment\n\nline 3\n\n\n\nline 4\n\n}";
        let expected = "line 1\nline 2\n\n# Comment\nline 3\nline 4\n}";
        assert_eq!(reduce_blank_lines(input), expected);

        let input_with_comment_after_brace = "line 1\n\n}\n# Comment\nline 2";
        let expected_with_comment_after_brace = "line 1\n}\n\n# Comment\nline 2";
        assert_eq!(
            reduce_blank_lines(input_with_comment_after_brace),
            expected_with_comment_after_brace
        );

        let input_with_comment_after_opening_brace = "{\n# Comment\nline 1";
        let expected_with_comment_after_opening_brace = "{\n# Comment\nline 1";
        assert_eq!(
            reduce_blank_lines(input_with_comment_after_opening_brace),
            expected_with_comment_after_opening_brace
        );

        let input_with_consecutive_comments = "# Comment 1\n\n# Comment 2\nline 1";
        let expected_with_consecutive_comments = "\n\n# Comment 1\n# Comment 2\nline 1";
        assert_eq!(
            reduce_blank_lines(input_with_consecutive_comments),
            expected_with_consecutive_comments
        );
    }
}
