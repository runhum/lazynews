use std::time::{SystemTime, UNIX_EPOCH};

use crate::hn::Comment;
use ratatui::{
    style::{Color, Style, Stylize},
    text::{Line, Span},
};

pub const POST_SELECTED_COLOR: Color = Color::Rgb(255, 149, 0);
pub const POST_META_COLOR: Color = Color::Rgb(140, 140, 140);
pub const COMMENT_AUTHOR_COLOR: Color = Color::Rgb(255, 149, 0);
pub const COMMENT_TEXT_COLOR: Color = Color::Rgb(225, 225, 225);
pub const COMMENT_QUOTE_COLOR: Color = POST_META_COLOR;
pub const COMMENT_INDENT_COLOR: Color = Color::Rgb(90, 90, 90);
pub const COMMENT_BORDER_COLOR: Color = Color::Rgb(255, 149, 0);
pub const SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

pub fn instructions_line(comments_open: bool, loading: bool, spinner: &str) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    let key =
        |label: &'static str| Span::styled(label, Style::new().fg(POST_SELECTED_COLOR).bold());

    if comments_open {
        spans.extend([
            "Scroll ".into(),
            key("<Up/Down>"),
            " Next sibling ".into(),
            key("<N>"),
            " Open ".into(),
            key("<O>"),
            " Close ".into(),
            key("<Esc>"),
            " Quit ".into(),
            key("<Q> "),
        ]);
    } else {
        if loading {
            spans.extend([
                "Refreshing ".yellow().bold(),
                Span::styled(spinner.to_string(), Style::new().yellow().bold()),
                " ".into(),
            ]);
        } else {
            spans.extend(["Refresh ".into(), key("<R>"), " ".into()]);
        }
        spans.extend([
            "Move ".into(),
            key("<Up/Down>"),
            " Comments ".into(),
            key("<Enter>"),
            " Open ".into(),
            key("<O>"),
            " Quit ".into(),
            key("<Q> "),
        ]);
    }

    Line::from(spans)
}

pub fn comment_lines(
    spinner: &str,
    content_width: usize,
    comments_for_post_id: Option<u64>,
    comments_loading: bool,
    comments_notice: Option<&str>,
    comments_error: Option<&str>,
    comments: &[Comment],
) -> (Vec<Line<'static>>, Vec<u16>) {
    if comments_for_post_id.is_none() {
        return (
            vec![Line::from("Press Enter on a post to load comments.")],
            Vec::new(),
        );
    }

    if comments_loading {
        return (
            vec![Line::from(format!("Loading comments {spinner}"))],
            Vec::new(),
        );
    }

    if let Some(message) = comments_notice {
        return (
            vec![Line::from(message.to_string()).style(Style::new().fg(POST_META_COLOR))],
            Vec::new(),
        );
    }

    if let Some(err) = comments_error {
        return (
            vec![Line::from(format!("Failed to load comments: {err}"))],
            Vec::new(),
        );
    }

    if comments.is_empty() {
        return (vec![Line::from("No comments found.")], Vec::new());
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut comment_start_lines: Vec<u16> = Vec::with_capacity(comments.len());

    for comment in comments {
        comment_start_lines.push(lines.len() as u16);
        let (header_prefix, body_prefix) = tree_prefix(comment);

        let mut header_spans: Vec<Span> = Vec::new();
        if !header_prefix.is_empty() {
            header_spans.push(Span::styled(
                header_prefix,
                Style::new().fg(COMMENT_INDENT_COLOR),
            ));
        }
        header_spans.push(Span::styled(
            comment.author.clone(),
            Style::new().fg(COMMENT_AUTHOR_COLOR).bold(),
        ));
        header_spans.push(Span::styled(" • ", Style::new().fg(POST_META_COLOR)));
        header_spans.push(Span::styled(
            format_age(comment.published_at),
            Style::new().fg(POST_META_COLOR),
        ));
        lines.push(Line::from(header_spans));

        for comment_line in comment.text.lines() {
            let is_quote = comment_line.trim_start().starts_with('>');
            let text_style = if is_quote {
                Style::new().fg(COMMENT_QUOTE_COLOR)
            } else {
                Style::new().fg(COMMENT_TEXT_COLOR)
            };

            let prefix_width = body_prefix.chars().count();
            let text_width = content_width.saturating_sub(prefix_width).max(1);
            let wrapped_segments = wrap_text(comment_line, text_width);

            for segment in wrapped_segments {
                let mut body_spans: Vec<Span> = Vec::new();
                if !body_prefix.is_empty() {
                    body_spans.push(Span::styled(
                        body_prefix.clone(),
                        Style::new().fg(COMMENT_INDENT_COLOR),
                    ));
                }
                body_spans.push(Span::styled(segment, text_style));
                lines.push(Line::from(body_spans));
            }
        }
    }

    (lines, comment_start_lines)
}

pub fn format_age(unix_seconds: u64) -> String {
    if unix_seconds == 0 {
        return "-".into();
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());

    let elapsed = now.saturating_sub(unix_seconds);

    match elapsed {
        0..=59 => format!("{elapsed}s ago"),
        60..=3_599 => format!("{}m ago", elapsed / 60),
        3_600..=86_399 => format!("{}h ago", elapsed / 3_600),
        86_400..=604_799 => format!("{}d ago", elapsed / 86_400),
        604_800..=2_591_999 => format!("{}w ago", elapsed / 604_800),
        2_592_000..=31_535_999 => format!("{}mo ago", elapsed / 2_592_000),
        _ => format!("{}y ago", elapsed / 31_536_000),
    }
}

fn tree_prefix(comment: &Comment) -> (String, String) {
    let mut header_prefix = String::new();
    let mut body_prefix = if comment.depth == 0 {
        "   ".to_string()
    } else {
        String::new()
    };

    for (level, has_next) in comment.ancestor_has_next_sibling.iter().enumerate() {
        if level == 0 {
            header_prefix.push_str("   ");
            body_prefix.push_str("   ");
            continue;
        }
        if *has_next {
            header_prefix.push_str("│  ");
            body_prefix.push_str("│  ");
        } else {
            header_prefix.push_str("   ");
            body_prefix.push_str("   ");
        }
    }

    if comment.depth > 0 {
        if comment.is_last_sibling {
            header_prefix.push_str("└─ ");
            body_prefix.push_str("   ");
        } else {
            header_prefix.push_str("├─ ");
            body_prefix.push_str("│  ");
        }
    }

    (header_prefix, body_prefix)
}

fn wrap_text(input: &str, width: usize) -> Vec<String> {
    if input.is_empty() {
        return vec![String::new()];
    }

    if width == 0 {
        return vec![String::new()];
    }

    let words: Vec<&str> = input.split_whitespace().collect();
    if words.is_empty() {
        return vec![String::new()];
    }

    let mut wrapped: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for word in words {
        let mut remaining = word;

        loop {
            if remaining.is_empty() {
                break;
            }

            let word_len = remaining.chars().count();
            if word_len <= width {
                let next_len = if current_len == 0 {
                    word_len
                } else {
                    current_len + 1 + word_len
                };

                if next_len <= width {
                    if current_len > 0 {
                        current.push(' ');
                    }
                    current.push_str(remaining);
                    current_len = next_len;
                } else {
                    if !current.is_empty() {
                        wrapped.push(std::mem::take(&mut current));
                    }
                    current.push_str(remaining);
                    current_len = word_len;
                }
                break;
            }

            if !current.is_empty() {
                wrapped.push(std::mem::take(&mut current));
                current_len = 0;
            }

            let chunk: String = remaining.chars().take(width).collect();
            let chunk_len = chunk.len();
            wrapped.push(chunk);
            remaining = &remaining[chunk_len..];
        }
    }

    if !current.is_empty() {
        wrapped.push(current);
    }

    if wrapped.is_empty() {
        vec![String::new()]
    } else {
        wrapped
    }
}
