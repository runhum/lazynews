use crate::hn::Comment;

pub fn current_index_from_scroll(
    comment_start_lines: &[u16],
    comments_len: usize,
    comments_scroll: u16,
) -> Option<usize> {
    if comments_len == 0 || comment_start_lines.is_empty() {
        return None;
    }

    let mut current = 0usize;
    for (index, line) in comment_start_lines.iter().enumerate() {
        if *line > comments_scroll {
            break;
        }
        current = index;
    }

    Some(current.min(comments_len.saturating_sub(1)))
}

pub fn next_sibling_or_outer_index(comments: &[Comment], current_index: usize) -> Option<usize> {
    let current_depth = comments.get(current_index)?.depth;

    for (next_index, comment) in comments.iter().enumerate().skip(current_index + 1) {
        if comment.depth == current_depth || comment.depth < current_depth {
            return Some(next_index);
        }
    }

    None
}

pub fn previous_sibling_or_parent_index(
    comments: &[Comment],
    current_index: usize,
) -> Option<usize> {
    let current_depth = comments.get(current_index)?.depth;

    for prev_index in (0..current_index).rev() {
        let depth = comments[prev_index].depth;
        if depth < current_depth {
            break;
        }

        if depth == current_depth {
            return Some(prev_index);
        }
    }

    nearest_parent_index(comments, current_index)
}

pub fn next_comment_index(comments_len: usize, current_index: usize) -> Option<usize> {
    if current_index + 1 < comments_len {
        Some(current_index + 1)
    } else {
        None
    }
}

pub fn previous_comment_index(current_index: usize) -> Option<usize> {
    if current_index > 0 {
        Some(current_index - 1)
    } else {
        None
    }
}

fn nearest_parent_index(comments: &[Comment], current_index: usize) -> Option<usize> {
    let current_depth = comments.get(current_index)?.depth;
    if current_depth == 0 {
        return None;
    }

    (0..current_index)
        .rev()
        .find(|&index| comments[index].depth < current_depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(depth: usize) -> Comment {
        Comment {
            author: "a".to_string(),
            text: "t".to_string(),
            published_at: 0,
            depth,
            ancestor_has_next_sibling: Vec::new(),
            is_last_sibling: true,
        }
    }

    #[test]
    fn current_index_clamps_to_comment_len() {
        assert_eq!(current_index_from_scroll(&[0, 2, 4], 2, 10), Some(1));
    }

    #[test]
    fn sibling_navigation_prefers_same_depth_then_outer() {
        let comments = vec![comment(0), comment(1), comment(2), comment(1), comment(0)];
        assert_eq!(next_sibling_or_outer_index(&comments, 1), Some(3));
        assert_eq!(next_sibling_or_outer_index(&comments, 3), Some(4));
    }

    #[test]
    fn previous_navigation_finds_sibling_or_parent() {
        let comments = vec![comment(0), comment(1), comment(2), comment(1), comment(0)];
        assert_eq!(previous_sibling_or_parent_index(&comments, 3), Some(1));
        assert_eq!(previous_sibling_or_parent_index(&comments, 2), Some(1));
    }
}
