use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalKeyAction {
    Exit,
    FocusNextPane,
    FocusPreviousPane,
    PaneShortcut(char),
    Refresh,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedsKeyAction {
    SelectPrevious,
    SelectNext,
    FocusPosts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostsKeyAction {
    SelectPrevious,
    SelectNextAndLoadMore,
    BookmarkSelected,
    OpenComments,
    OpenPost,
    CloseComments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentsKeyAction {
    Close,
    BookmarkPost,
    OpenPost,
    JumpPrevious,
    JumpNext,
    JumpPreviousSibling,
    JumpNextSibling,
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollHome,
    ScrollEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookmarksKeyAction {
    Expand,
    Close,
    BookmarkSelected,
    SelectPrevious,
    SelectNext,
    OpenComments,
    OpenPost,
    OpenAll,
    Delete,
}

pub fn map_global_action(key_event: KeyEvent) -> Option<GlobalKeyAction> {
    if matches!(key_event.code, KeyCode::Char('c'))
        && key_event.modifiers.contains(KeyModifiers::CONTROL)
    {
        return Some(GlobalKeyAction::Exit);
    }

    match key_event.code {
        KeyCode::Tab => Some(GlobalKeyAction::FocusNextPane),
        KeyCode::BackTab => Some(GlobalKeyAction::FocusPreviousPane),
        KeyCode::Char(shortcut @ '1'..='4') => Some(GlobalKeyAction::PaneShortcut(shortcut)),
        KeyCode::Char('r') | KeyCode::Char('R') => Some(GlobalKeyAction::Refresh),
        KeyCode::Char('q') => Some(GlobalKeyAction::Quit),
        _ => None,
    }
}

pub fn map_feeds_action(key_code: KeyCode) -> Option<FeedsKeyAction> {
    match key_code {
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('H') => {
            Some(FeedsKeyAction::SelectPrevious)
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('L') => {
            Some(FeedsKeyAction::SelectNext)
        }
        KeyCode::Enter => Some(FeedsKeyAction::FocusPosts),
        _ => None,
    }
}

pub fn map_posts_action(key_code: KeyCode, comments_open: bool) -> Option<PostsKeyAction> {
    match key_code {
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => {
            Some(PostsKeyAction::SelectPrevious)
        }
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => {
            Some(PostsKeyAction::SelectNextAndLoadMore)
        }
        KeyCode::Char('b') | KeyCode::Char('B') => Some(PostsKeyAction::BookmarkSelected),
        KeyCode::Enter => Some(PostsKeyAction::OpenComments),
        KeyCode::Char('o') | KeyCode::Char('O') => Some(PostsKeyAction::OpenPost),
        KeyCode::Esc if comments_open => Some(PostsKeyAction::CloseComments),
        _ => None,
    }
}

pub fn map_comments_action(key_code: KeyCode) -> Option<CommentsKeyAction> {
    match key_code {
        KeyCode::Esc => Some(CommentsKeyAction::Close),
        KeyCode::Char('b') | KeyCode::Char('B') => Some(CommentsKeyAction::BookmarkPost),
        KeyCode::Char('o') | KeyCode::Char('O') => Some(CommentsKeyAction::OpenPost),
        KeyCode::Up => Some(CommentsKeyAction::JumpPrevious),
        KeyCode::Down => Some(CommentsKeyAction::JumpNext),
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('H') => {
            Some(CommentsKeyAction::JumpPreviousSibling)
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('L') => {
            Some(CommentsKeyAction::JumpNextSibling)
        }
        KeyCode::Char('k') | KeyCode::Char('K') => Some(CommentsKeyAction::ScrollUp),
        KeyCode::Char('j') | KeyCode::Char('J') => Some(CommentsKeyAction::ScrollDown),
        KeyCode::PageUp => Some(CommentsKeyAction::ScrollPageUp),
        KeyCode::PageDown => Some(CommentsKeyAction::ScrollPageDown),
        KeyCode::Home => Some(CommentsKeyAction::ScrollHome),
        KeyCode::End => Some(CommentsKeyAction::ScrollEnd),
        _ => None,
    }
}

pub fn map_bookmarks_action(
    key_code: KeyCode,
    bookmarks_collapsed: bool,
) -> Option<BookmarksKeyAction> {
    if bookmarks_collapsed {
        return match key_code {
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('L') => {
                Some(BookmarksKeyAction::Expand)
            }
            KeyCode::Esc => Some(BookmarksKeyAction::Close),
            _ => None,
        };
    }

    match key_code {
        KeyCode::Char('b') | KeyCode::Char('B') => Some(BookmarksKeyAction::BookmarkSelected),
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => {
            Some(BookmarksKeyAction::SelectPrevious)
        }
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => {
            Some(BookmarksKeyAction::SelectNext)
        }
        KeyCode::Enter => Some(BookmarksKeyAction::OpenComments),
        KeyCode::Char('o') | KeyCode::Char('O') => Some(BookmarksKeyAction::OpenPost),
        KeyCode::Char('a') | KeyCode::Char('A') => Some(BookmarksKeyAction::OpenAll),
        KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Delete | KeyCode::Backspace => {
            Some(BookmarksKeyAction::Delete)
        }
        KeyCode::Esc => Some(BookmarksKeyAction::Close),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_mapping_handles_ctrl_c_and_shortcuts() {
        assert_eq!(
            map_global_action(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(GlobalKeyAction::Exit)
        );
        assert_eq!(
            map_global_action(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            Some(GlobalKeyAction::PaneShortcut('2'))
        );
        assert_eq!(
            map_global_action(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn bookmarks_mapping_depends_on_collapsed_state() {
        assert_eq!(
            map_bookmarks_action(KeyCode::Enter, true),
            Some(BookmarksKeyAction::Expand)
        );
        assert_eq!(map_bookmarks_action(KeyCode::Down, true), None);
        assert_eq!(
            map_bookmarks_action(KeyCode::Down, false),
            Some(BookmarksKeyAction::SelectNext)
        );
        assert_eq!(
            map_bookmarks_action(KeyCode::Char('a'), false),
            Some(BookmarksKeyAction::OpenAll)
        );
    }
}
