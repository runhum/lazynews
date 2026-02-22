use std::time::{SystemTime, UNIX_EPOCH};

use crate::hn::Comment;
use ratatui::{
    style::{Color, Style, Stylize},
    text::{Line, Span},
};

pub const POST_SELECTED_COLOR: Color = Color::Rgb(255, 149, 0);
pub const PANE_SHORTCUT_COLOR: Color = POST_SELECTED_COLOR;
pub const POST_META_COLOR: Color = Color::Rgb(140, 140, 140);
pub const COMMENT_AUTHOR_COLOR: Color = Color::Rgb(255, 149, 0);
pub const COMMENT_TEXT_COLOR: Color = Color::Rgb(225, 225, 225);
pub const COMMENT_QUOTE_COLOR: Color = POST_META_COLOR;
pub const COMMENT_INDENT_COLOR: Color = Color::Rgb(90, 90, 90);
pub const COMMENT_BORDER_COLOR: Color = Color::Rgb(255, 149, 0);
pub const SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionsPane {
    Feeds,
    Bookmarks,
    Posts,
    Comments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Feeds,
    Bookmarks,
    Posts,
    Comments,
}

pub fn instructions_pane_for(pane: Pane) -> InstructionsPane {
    match pane {
        Pane::Feeds => InstructionsPane::Feeds,
        Pane::Bookmarks => InstructionsPane::Bookmarks,
        Pane::Posts => InstructionsPane::Posts,
        Pane::Comments => InstructionsPane::Comments,
    }
}

pub fn pane_border_style(active_pane: Pane, pane: Pane) -> Style {
    if active_pane == pane {
        Style::new().fg(COMMENT_BORDER_COLOR)
    } else {
        Style::new().fg(POST_META_COLOR)
    }
}

pub fn pane_title_with_shortcut(
    title: impl Into<String>,
    shortcut: char,
    active_pane: Pane,
    pane: Pane,
) -> Line<'static> {
    let title = title.into();
    let shortcut_style = if active_pane == pane {
        Style::default()
    } else {
        Style::new().fg(PANE_SHORTCUT_COLOR).bold()
    };

    Line::from(vec![
        Span::raw(format!("{title} (")),
        Span::styled(shortcut.to_string(), shortcut_style),
        Span::raw(")"),
    ])
}

pub fn instructions_line(
    active_pane: InstructionsPane,
    comments_open: bool,
    bookmarks_visible: bool,
    bookmarks_collapsed: bool,
    loading: bool,
    spinner: &str,
) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    let key =
        |label: &'static str| Span::styled(label, Style::new().fg(POST_SELECTED_COLOR).bold());

    spans.extend(["Pane ".into(), key("<Tab/Shift-Tab>"), " ".into()]);

    match active_pane {
        InstructionsPane::Feeds => spans.extend([
            "Switch feed ".into(),
            key("<Left/Right/H/L>"),
            " Quit ".into(),
            key("<Q> "),
        ]),
        InstructionsPane::Posts => {
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
                key("<Up/Down/J/K>"),
                " Bookmark ".into(),
                key("<B>"),
                " Comments ".into(),
                key("<Enter>"),
                " Open ".into(),
                key("<O>"),
            ]);
            if comments_open {
                spans.extend([" Close comments ".into(), key("<Esc>")]);
            }
            spans.extend([" Quit ".into(), key("<Q> ")]);
        }
        InstructionsPane::Bookmarks => {
            if bookmarks_collapsed {
                spans.extend([
                    "Expand ".into(),
                    key("<Enter/Right/L>"),
                    " Close ".into(),
                    key("<Esc>"),
                ]);
            } else {
                spans.extend([
                    "Move ".into(),
                    key("<Up/Down/J/K>"),
                    " Comments ".into(),
                    key("<Enter>"),
                    " Open ".into(),
                    key("<O>"),
                    " Open all ".into(),
                    key("<A>"),
                    " Delete ".into(),
                    key("<D/Del/Bksp>"),
                    " Close ".into(),
                    key("<Esc>"),
                ]);
            }
            spans.extend([" Quit ".into(), key("<Q> ")]);
        }
        InstructionsPane::Comments => {
            spans.extend([
                "Navigate ".into(),
                key("<Up/Down/Left/Right>"),
                " Bookmark ".into(),
                key("<B>"),
                " Open ".into(),
                key("<O>"),
                " Close ".into(),
                key("<Esc>"),
                " Quit ".into(),
                key("<Q> "),
            ]);
        }
    }

    if matches!(active_pane, InstructionsPane::Bookmarks) && !bookmarks_visible {
        spans.extend([" ".into(), "(No bookmarks yet)".into()]);
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

    if comments_loading && comments.is_empty() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn as_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    fn sample_comment(
        author: &str,
        text: &str,
        depth: usize,
        ancestor_has_next_sibling: Vec<bool>,
        is_last_sibling: bool,
    ) -> Comment {
        Comment {
            author: author.to_string(),
            text: text.to_string(),
            published_at: 1,
            depth,
            ancestor_has_next_sibling,
            is_last_sibling,
        }
    }

    #[test]
    fn wrap_text_wraps_by_word_when_it_fits() {
        let wrapped = wrap_text("alpha beta gamma", 10);
        assert_eq!(wrapped, vec!["alpha beta", "gamma"]);
    }

    #[test]
    fn wrap_text_splits_long_words_into_chunks() {
        let wrapped = wrap_text("abcdefgh ij", 4);
        assert_eq!(wrapped, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn wrap_text_splits_unicode_words_without_panicking() {
        let wrapped = wrap_text("åäö🙂🙂", 2);
        assert_eq!(wrapped, vec!["åä", "ö🙂", "🙂"]);
    }

    #[test]
    fn wrap_text_returns_blank_for_zero_width() {
        let wrapped = wrap_text("alpha beta", 0);
        assert_eq!(wrapped, vec![String::new()]);
    }

    #[test]
    fn comment_lines_returns_placeholder_states() {
        let (lines, starts) = comment_lines("|", 40, None, false, None, None, &[]);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            as_text(&lines[0]),
            "Press Enter on a post to load comments."
        );
        assert!(starts.is_empty());

        let (lines, starts) = comment_lines("|", 40, Some(1), true, None, None, &[]);
        assert_eq!(lines.len(), 1);
        assert_eq!(as_text(&lines[0]), "Loading comments |");
        assert!(starts.is_empty());

        let (lines, starts) =
            comment_lines("|", 40, Some(1), false, Some("no comments"), None, &[]);
        assert_eq!(lines.len(), 1);
        assert_eq!(as_text(&lines[0]), "no comments");
        assert!(starts.is_empty());

        let (lines, starts) = comment_lines("|", 40, Some(1), false, None, Some("boom"), &[]);
        assert_eq!(lines.len(), 1);
        assert_eq!(as_text(&lines[0]), "Failed to load comments: boom");
        assert!(starts.is_empty());

        let (lines, starts) = comment_lines("|", 40, Some(1), false, None, None, &[]);
        assert_eq!(lines.len(), 1);
        assert_eq!(as_text(&lines[0]), "No comments found.");
        assert!(starts.is_empty());
    }

    #[test]
    fn comment_lines_renders_cached_comments_while_refreshing() {
        let comments = vec![sample_comment("alice", "cached text", 0, vec![], true)];
        let (lines, starts) = comment_lines("|", 40, Some(1), true, None, None, &comments);
        let rendered: Vec<String> = lines.iter().map(as_text).collect();

        assert_eq!(starts, vec![0]);
        assert!(rendered.iter().any(|line| line.contains("alice")));
        assert!(rendered.iter().any(|line| line.contains("cached text")));
        assert!(
            !rendered
                .iter()
                .any(|line| line.contains("Loading comments"))
        );
    }

    #[test]
    fn comment_lines_tracks_comment_start_lines_and_tree_prefixes() {
        let comments = vec![
            sample_comment("alice", "hello world", 0, vec![], false),
            sample_comment("bob", "> quoted\nreply", 1, vec![true], true),
        ];

        let (lines, starts) = comment_lines("|", 24, Some(42), false, None, None, &comments);
        let rendered: Vec<String> = lines.iter().map(as_text).collect();

        assert_eq!(starts, vec![0, 2]);
        assert_eq!(rendered.len(), 5);
        assert!(rendered[0].contains("alice"));
        assert!(rendered[2].contains("bob"));
        assert!(rendered[2].contains("└─ "));
        assert!(rendered[3].contains("> quoted"));
        assert!(rendered[4].contains("reply"));
    }

    #[test]
    fn format_age_returns_dash_for_zero_timestamp() {
        assert_eq!(format_age(0), "-");
    }

    #[test]
    fn instructions_show_tab_navigation_and_hide_numeric_shortcuts() {
        let panes = [
            InstructionsPane::Feeds,
            InstructionsPane::Bookmarks,
            InstructionsPane::Posts,
            InstructionsPane::Comments,
        ];

        for pane in panes {
            let line = instructions_line(pane, true, true, false, false, "|");
            let text = as_text(&line);

            assert!(text.contains("Pane"));
            assert!(text.contains("<Tab/Shift-Tab>"));
            assert!(!text.contains("<1>"));
            assert!(!text.contains("<2>"));
            assert!(!text.contains("<3>"));
            assert!(!text.contains("<4>"));
        }
    }

    #[test]
    fn refresh_hint_shows_only_in_posts_pane() {
        let line = instructions_line(InstructionsPane::Posts, false, false, false, false, "|");
        let text = as_text(&line);
        assert!(text.contains("Refresh"));
        assert!(text.contains("<R>"));

        let line = instructions_line(InstructionsPane::Feeds, false, false, false, false, "|");
        let text = as_text(&line);
        assert!(!text.contains("Refresh"));
        assert!(!text.contains("<R>"));
    }

    #[test]
    fn collapsed_bookmarks_instructions_only_show_collapsed_actions() {
        let line = instructions_line(InstructionsPane::Bookmarks, true, true, true, false, "|");
        let text = as_text(&line);

        assert!(text.contains("<Enter/Right/L>"));
        assert!(text.contains("<Esc>"));
        assert!(!text.contains("<Up/Down/J/K>"));
        assert!(!text.contains("<D/Del/Bksp>"));
    }

    #[test]
    fn expanded_bookmarks_instructions_include_open_all() {
        let line = instructions_line(InstructionsPane::Bookmarks, true, true, false, false, "|");
        let text = as_text(&line);

        assert!(text.contains("<A>"));
        assert!(text.contains("Open all"));
    }
}
