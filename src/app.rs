use crate::{
    comments_nav::{
        current_index_from_scroll, next_comment_index, next_sibling_or_outer_index,
        previous_comment_index, previous_sibling_or_parent_index,
    },
    event::{AppEvent, Event, EventHandler, PostsFetchMode, PostsFetchResult},
    hn::{Comment, HackerNewsApi, Item, StoryFeed},
    input::{
        BookmarksKeyAction, CommentsKeyAction, FeedsKeyAction, GlobalKeyAction, PostsKeyAction,
        map_bookmarks_action, map_comments_action, map_feeds_action, map_global_action,
        map_posts_action,
    },
    ui::{
        POST_META_COLOR, POST_SELECTED_COLOR, Pane, SPINNER_FRAMES,
        comment_lines as build_comment_lines, format_age, instructions_line, instructions_pane_for,
        pane_border_style, pane_title_with_shortcut,
    },
};
use chrono::Local;
use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    style::{Style, Stylize},
    symbols::border,
    text::Line,
    widgets::{Block, List, ListItem, ListState, Paragraph, Tabs},
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub struct App {
    running: bool,
    hn_client: HackerNewsApi,
    events: EventHandler,
    loading_frame: usize,
    story_ids: Vec<u64>,
    next_story_index: usize,
    has_more_posts: bool,
    posts: Vec<Post>,
    bookmarks: Vec<Post>,
    posts_notice: Option<String>,
    selected_feed: FeedTab,
    feed_cache: HashMap<FeedTab, CachedFeed>,
    next_posts_request_id: u64,
    active_posts_request_id: Option<u64>,
    posts_request_cancel: Option<CancellationToken>,
    last_fetched: Option<String>,
    pub loading: bool,
    list_state: ListState,
    bookmarks_state: ListState,
    focus_pane: Pane,
    comments_open: bool,
    comments: Vec<Comment>,
    comments_for_post_id: Option<u64>,
    comments_loading: bool,
    comments_error: Option<String>,
    comments_notice: Option<String>,
    comments_scroll: u16,
    comments_viewport_height: usize,
    comment_line_count: usize,
    comment_start_lines: Vec<u16>,
    comments_cache: HashMap<u64, CachedComments>,
    bookmarks_collapsed: bool,
}

#[derive(Debug, Clone)]
struct Post {
    id: u64,
    title: String,
    url: String,
    post_type: PostType,
    points: u64,
    comments: u64,
    author: String,
    published_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostType {
    Story,
    Job,
}

impl PostType {
    fn from_kind(kind: Option<&str>) -> Option<Self> {
        match kind {
            Some("story") => Some(Self::Story),
            Some("job") => Some(Self::Job),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FeedTab {
    Top,
    New,
    Ask,
    Show,
    Jobs,
    Best,
}

#[derive(Debug, Clone)]
struct CachedFeed {
    story_ids: Vec<u64>,
    next_story_index: usize,
    has_more_posts: bool,
    posts: Vec<Post>,
    selected_index: Option<usize>,
    last_fetched: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedComments {
    comments: Vec<Comment>,
    fetched_at: Instant,
}

impl FeedTab {
    const ALL: [Self; 6] = [
        Self::Top,
        Self::New,
        Self::Ask,
        Self::Show,
        Self::Jobs,
        Self::Best,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::New => "new",
            Self::Ask => "ask",
            Self::Show => "show",
            Self::Jobs => "jobs",
            Self::Best => "best",
        }
    }

    fn posts_title(self) -> &'static str {
        match self {
            Self::Top => "Top Stories",
            Self::New => "New Stories",
            Self::Ask => "Ask HN",
            Self::Show => "Show HN",
            Self::Jobs => "Jobs",
            Self::Best => "Best Stories",
        }
    }

    fn api_feed(self) -> StoryFeed {
        match self {
            Self::Top => StoryFeed::Top,
            Self::New => StoryFeed::New,
            Self::Ask => StoryFeed::Ask,
            Self::Show => StoryFeed::Show,
            Self::Jobs => StoryFeed::Jobs,
            Self::Best => StoryFeed::Best,
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }

    fn from_index(index: usize) -> Self {
        Self::ALL[index % Self::ALL.len()]
    }
}

const POSTS_PAGE_SIZE: usize = 30;
const LOAD_MORE_TRIGGER_NUMERATOR: usize = 3;
const LOAD_MORE_TRIGGER_DENOMINATOR: usize = 4;
const COMMENTS_CACHE_REFRESH_AFTER_SECS: u64 = 90;

impl App {
    pub fn new() -> Self {
        Self {
            running: true,
            hn_client: HackerNewsApi::new(),
            events: EventHandler::new(),
            loading_frame: 0,
            story_ids: Vec::new(),
            next_story_index: 0,
            has_more_posts: true,
            posts: Vec::new(),
            bookmarks: Vec::new(),
            posts_notice: None,
            selected_feed: FeedTab::Top,
            feed_cache: HashMap::new(),
            next_posts_request_id: 0,
            active_posts_request_id: None,
            posts_request_cancel: None,
            last_fetched: None,
            loading: false,
            list_state: ListState::default(),
            bookmarks_state: ListState::default(),
            focus_pane: Pane::Posts,
            comments_open: false,
            comments: Vec::new(),
            comments_for_post_id: None,
            comments_loading: false,
            comments_error: None,
            comments_notice: None,
            comments_scroll: 0,
            comments_viewport_height: 0,
            comment_line_count: 0,
            comment_start_lines: Vec::new(),
            comments_cache: HashMap::new(),
            bookmarks_collapsed: false,
        }
    }

    pub async fn run(&mut self, mut terminal: DefaultTerminal) -> Result<()> {
        self.events.send(AppEvent::Refresh);
        while self.running {
            terminal.draw(|frame| self.draw(frame))?;
            match self.events.next().await? {
                Event::App(app_event) => self.handle_app_event(app_event),
                Event::Tick => self.on_tick(),
                Event::Key(key_event) => self.handle_key_event(key_event)?,
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        self.ensure_focus_valid();
        let title = Line::from("lazynews".bold());
        let spinner = self.spinner_frame();
        let instructions = instructions_line(
            instructions_pane_for(self.focus_pane),
            self.comments_open,
            self.bookmarks_visible(),
            self.bookmarks_collapsed,
            self.loading,
            spinner,
        );

        let outer_block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered())
            .border_set(border::THICK);

        let content_area = outer_block.inner(frame.area());
        frame.render_widget(outer_block, frame.area());

        let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]);
        let areas = layout.split(content_area);
        self.render_feed_tabs(frame, areas[0]);

        if self.bookmarks_visible() {
            let bookmarks_width = self.bookmarks_width_percent();
            let layout = Layout::horizontal([
                Constraint::Percentage(bookmarks_width),
                Constraint::Percentage(100 - bookmarks_width),
            ]);
            let panes = layout.split(areas[1]);
            self.render_bookmarks_list(frame, panes[0]);

            if self.comments_open {
                let post_comment_layout =
                    Layout::horizontal([Constraint::Percentage(33), Constraint::Percentage(67)]);
                let post_comment_panes = post_comment_layout.split(panes[1]);
                self.render_posts_list(frame, post_comment_panes[0]);
                self.render_comments_pane(frame, post_comment_panes[1], spinner);
            } else {
                self.render_posts_list(frame, panes[1]);
            }
        } else if self.comments_open {
            let layout =
                Layout::horizontal([Constraint::Percentage(33), Constraint::Percentage(67)]);
            let panes = layout.split(areas[1]);
            self.render_posts_list(frame, panes[0]);
            self.render_comments_pane(frame, panes[1], spinner);
        } else {
            self.render_posts_list(frame, areas[1]);
        }
    }

    fn render_feed_tabs(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let titles = FeedTab::ALL.iter().map(|tab| tab.label());
        let block = Block::bordered()
            .title(pane_title_with_shortcut(
                "Feeds",
                '3',
                self.focus_pane,
                Pane::Feeds,
            ))
            .border_style(pane_border_style(self.focus_pane, Pane::Feeds));

        let tabs = Tabs::new(titles)
            .block(block)
            .select(self.selected_feed.index())
            .style(Style::new().fg(POST_META_COLOR))
            .highlight_style(Style::new().fg(POST_SELECTED_COLOR).bold())
            .divider("|");

        frame.render_widget(tabs, area);
    }

    fn render_posts_list(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let items: Vec<ListItem> = if let Some(notice) = self.posts_notice.as_deref() {
            vec![ListItem::new(
                Line::from(notice.to_string()).style(Style::new().fg(POST_META_COLOR)),
            )]
        } else if self.posts.is_empty() {
            if self.loading {
                vec![ListItem::new(Line::from(format!(
                    "Refreshing {}",
                    self.spinner_frame()
                )))]
            } else {
                vec![ListItem::new(Line::from("No posts loaded"))]
            }
        } else {
            let selected = self.list_state.selected();
            let post_rank_width = self.posts.len().to_string().len().max(1);
            let meta_indent = " ".repeat(post_rank_width + 2);
            self.posts
                .iter()
                .enumerate()
                .map(|(i, post)| {
                    let title_style = if selected == Some(i) {
                        Style::new().fg(POST_SELECTED_COLOR).bold()
                    } else {
                        Style::new()
                    };
                    let title_line = Line::from(format!(
                        "{:>width$}. {}",
                        i + 1,
                        post.title,
                        width = post_rank_width
                    ))
                    .style(title_style);
                    let meta_text = match post.post_type {
                        PostType::Job => format!(
                            "{}job • {} points • by {} • {}",
                            meta_indent,
                            post.points,
                            post.author,
                            format_age(post.published_at)
                        ),
                        PostType::Story => format!(
                            "{}{} points • {} comments • by {} • {}",
                            meta_indent,
                            post.points,
                            post.comments,
                            post.author,
                            format_age(post.published_at)
                        ),
                    };
                    let meta_line = Line::from(meta_text).style(Style::new().fg(POST_META_COLOR));
                    ListItem::new(vec![title_line, meta_line])
                })
                .collect()
        };

        let mut block = Block::bordered().title(pane_title_with_shortcut(
            self.selected_feed.posts_title(),
            '2',
            self.focus_pane,
            Pane::Posts,
        ));
        block = block.border_style(pane_border_style(self.focus_pane, Pane::Posts));
        if let Some(last_fetched) = self.last_fetched.as_deref() {
            block = block.title(
                Line::from(format!("last fetched {last_fetched}"))
                    .right_aligned()
                    .style(Style::new().fg(POST_META_COLOR)),
            );
        }

        let list = List::new(items).block(block).highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_bookmarks_list(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let items: Vec<ListItem> = if self.bookmarks.is_empty() {
            vec![ListItem::new(
                Line::from("Press b on a post to bookmark it.")
                    .style(Style::new().fg(POST_META_COLOR)),
            )]
        } else {
            self.bookmarks
                .iter()
                .map(|post| ListItem::new(Line::from(post.title.clone())))
                .collect()
        };

        let block = Block::bordered()
            .title(pane_title_with_shortcut(
                "Bookmarks",
                '1',
                self.focus_pane,
                Pane::Bookmarks,
            ))
            .border_style(pane_border_style(self.focus_pane, Pane::Bookmarks));
        let is_focused = self.focus_pane == Pane::Bookmarks;
        let list = if is_focused {
            List::new(items)
                .block(block)
                .highlight_symbol("> ")
                .highlight_style(Style::new().fg(POST_SELECTED_COLOR).bold())
        } else {
            List::new(items).block(block)
        };

        if is_focused {
            frame.render_stateful_widget(list, area, &mut self.bookmarks_state);
        } else {
            frame.render_widget(list, area);
        }
    }

    fn render_comments_pane(
        &mut self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        spinner: &str,
    ) {
        let comments_title = self
            .comments_post()
            .map(|post| format!("{} | {} comments", post.title, post.comments))
            .unwrap_or_else(|| "Comments".to_string());

        let content_width = area.width.saturating_sub(2) as usize;
        let (lines, comment_start_lines) = build_comment_lines(
            spinner,
            content_width,
            self.comments_for_post_id,
            self.comments_loading,
            self.comments_notice.as_deref(),
            self.comments_error.as_deref(),
            &self.comments,
        );
        self.comment_start_lines = comment_start_lines;
        self.comment_line_count = lines.len();
        self.comments_viewport_height = area.height.saturating_sub(2) as usize;
        self.clamp_comments_scroll();

        let widget = Paragraph::new(lines)
            .block(
                Block::bordered()
                    .title(pane_title_with_shortcut(
                        comments_title,
                        '4',
                        self.focus_pane,
                        Pane::Comments,
                    ))
                    .border_style(pane_border_style(self.focus_pane, Pane::Comments)),
            )
            .scroll((self.comments_scroll, 0));

        frame.render_widget(widget, area);
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        if let Some(action) = map_global_action(key_event) {
            match action {
                GlobalKeyAction::Exit => self.exit(),
                GlobalKeyAction::FocusNextPane => self.focus_next_pane(),
                GlobalKeyAction::FocusPreviousPane => self.focus_previous_pane(),
                GlobalKeyAction::PaneShortcut(shortcut) => self.handle_pane_shortcut(shortcut),
                GlobalKeyAction::Refresh => self.events.send(AppEvent::Refresh),
                GlobalKeyAction::Quit => self.events.send(AppEvent::Quit),
            }
            return Ok(());
        }

        match self.focus_pane {
            Pane::Feeds => self.handle_feeds_key(key_event.code),
            Pane::Posts => self.handle_posts_key(key_event.code),
            Pane::Comments => self.handle_comments_key(key_event.code),
            Pane::Bookmarks => self.handle_bookmarks_key(key_event.code),
        }

        Ok(())
    }

    fn handle_feeds_key(&mut self, key_code: KeyCode) {
        if let Some(action) = map_feeds_action(key_code) {
            match action {
                FeedsKeyAction::SelectPrevious => self.select_previous_feed(),
                FeedsKeyAction::SelectNext => self.select_next_feed(),
                FeedsKeyAction::FocusPosts => self.set_focus_pane(Pane::Posts),
            }
        }
    }

    fn handle_pane_shortcut(&mut self, key: char) {
        match key {
            '1' => {
                if !self.bookmarks_visible() {
                    return;
                }

                if self.focus_pane == Pane::Bookmarks {
                    if self.bookmarks_collapsed {
                        self.open_bookmarks_pane();
                    } else {
                        self.bookmarks_collapsed = true;
                    }
                } else {
                    self.open_bookmarks_pane();
                }
            }
            '2' => {
                self.set_focus_pane(Pane::Posts);
            }
            '3' => {
                self.set_focus_pane(Pane::Feeds);
            }
            '4' => {
                if self.comments_open {
                    self.set_focus_pane(Pane::Comments);
                }
            }
            _ => {}
        }
        self.ensure_focus_valid();
    }

    fn handle_posts_key(&mut self, key_code: KeyCode) {
        if let Some(action) = map_posts_action(key_code, self.comments_open) {
            match action {
                PostsKeyAction::SelectPrevious => self.select_previous(),
                PostsKeyAction::SelectNextAndLoadMore => {
                    self.select_next();
                    self.load_more_posts();
                }
                PostsKeyAction::BookmarkSelected => self.bookmark_selected_post(),
                PostsKeyAction::OpenComments => self.open_comments_for_selected(),
                PostsKeyAction::OpenPost => self.open_selected_post(),
                PostsKeyAction::CloseComments => self.close_comments_view(),
            }
        }
    }

    fn handle_comments_key(&mut self, key_code: KeyCode) {
        if let Some(action) = map_comments_action(key_code) {
            match action {
                CommentsKeyAction::Close => self.close_comments_view(),
                CommentsKeyAction::BookmarkPost => self.bookmark_comments_post(),
                CommentsKeyAction::OpenPost => self.open_comments_post(),
                CommentsKeyAction::JumpPrevious => self.jump_to_previous_comment(),
                CommentsKeyAction::JumpNext => self.jump_to_next_comment(),
                CommentsKeyAction::JumpPreviousSibling => self.jump_to_previous_sibling_comment(),
                CommentsKeyAction::JumpNextSibling => self.jump_to_next_sibling_comment(),
                CommentsKeyAction::ScrollUp => self.scroll_comments_up(1),
                CommentsKeyAction::ScrollDown => self.scroll_comments_down(1),
                CommentsKeyAction::ScrollPageUp => {
                    self.scroll_comments_up(self.comment_page_step())
                }
                CommentsKeyAction::ScrollPageDown => {
                    self.scroll_comments_down(self.comment_page_step())
                }
                CommentsKeyAction::ScrollHome => self.comments_scroll = 0,
                CommentsKeyAction::ScrollEnd => self.comments_scroll = self.max_comment_scroll(),
            }
        }
    }

    fn handle_bookmarks_key(&mut self, key_code: KeyCode) {
        if let Some(action) = map_bookmarks_action(key_code, self.bookmarks_collapsed) {
            match action {
                BookmarksKeyAction::Expand => self.open_bookmarks_pane(),
                BookmarksKeyAction::Close => self.close_bookmarks_pane(),
                BookmarksKeyAction::BookmarkSelected => self.bookmark_selected_post(),
                BookmarksKeyAction::SelectPrevious => self.select_previous_bookmark(),
                BookmarksKeyAction::SelectNext => self.select_next_bookmark(),
                BookmarksKeyAction::OpenComments => self.select_post_from_bookmark(),
                BookmarksKeyAction::OpenPost => self.open_selected_bookmark(),
                BookmarksKeyAction::OpenAll => self.open_all_bookmarks(),
                BookmarksKeyAction::Delete => self.remove_selected_bookmark(),
            }
        }
    }

    fn exit(&mut self) {
        self.running = false;
    }

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Quit => self.exit(),
            AppEvent::Refresh => {
                self.posts_notice = None;
                self.reset_comments_state();
                self.refresh_posts();
            }
            AppEvent::PostsFetched { request_id, result } => {
                self.handle_posts_fetched(request_id, result)
            }
            AppEvent::LoadCommentsComplete { post_id, result } => {
                if !self.comments_open || self.comments_for_post_id != Some(post_id) {
                    return;
                }

                self.comments_loading = false;
                match result {
                    Ok(comments) => {
                        self.comments_cache.insert(
                            post_id,
                            CachedComments {
                                comments: comments.clone(),
                                fetched_at: Instant::now(),
                            },
                        );
                        self.comments = comments;
                        self.comments_error = None;
                        self.comments_notice = None;
                        self.comments_scroll = 0;
                        self.comment_start_lines.clear();
                    }
                    Err(err) => {
                        if self.comments_cache.contains_key(&post_id) {
                            self.comments_error = None;
                        } else {
                            self.comments.clear();
                            self.comments_error = Some(err);
                            self.comments_notice = None;
                            self.comment_start_lines.clear();
                        }
                    }
                }
            }
            AppEvent::OpenPost(url) => {
                let _ = webbrowser::open(&url);
            }
        }
    }

    fn on_tick(&mut self) {
        if self.loading || (self.comments_open && self.comments_loading) {
            self.loading_frame = self.loading_frame.wrapping_add(1);
        }
    }

    fn spinner_frame(&self) -> &'static str {
        SPINNER_FRAMES[self.loading_frame % SPINNER_FRAMES.len()]
    }

    fn current_hhmm() -> String {
        Local::now().format("%H:%M:%S").to_string()
    }

    fn begin_posts_request(&mut self) -> (u64, CancellationToken) {
        if let Some(cancel_token) = self.posts_request_cancel.take() {
            cancel_token.cancel();
        }

        self.next_posts_request_id = self.next_posts_request_id.wrapping_add(1).max(1);
        let request_id = self.next_posts_request_id;
        self.active_posts_request_id = Some(request_id);
        self.loading = true;
        self.loading_frame = 0;

        let cancel_token = CancellationToken::new();
        self.posts_request_cancel = Some(cancel_token.clone());
        (request_id, cancel_token)
    }

    fn refresh_posts(&mut self) {
        let (request_id, cancel_token) = self.begin_posts_request();
        if self.posts.is_empty() {
            self.story_ids.clear();
            self.next_story_index = 0;
            self.has_more_posts = true;
            self.list_state.select(None);
        }
        self.posts_notice = None;
        let feed = self.selected_feed.api_feed();

        let client = self.hn_client.clone();
        self.events.send_async(async move {
            let result: Result<PostsFetchResult, String> = tokio::select! {
                _ = cancel_token.cancelled() => Err("Cancelled".to_string()),
                result = async {
                    let story_ids = client.fetch_story_ids(feed).await?;
                    let next_story_index = story_ids.len().min(POSTS_PAGE_SIZE);
                    let page_ids: Vec<u64> = story_ids.iter().take(next_story_index).copied().collect();
                    let items = client.fetch_items_by_ids(&page_ids, feed).await?;

                    Ok(PostsFetchResult {
                        mode: PostsFetchMode::Replace,
                        story_ids: Some(story_ids),
                        items,
                        next_story_index,
                    })
                } => result.map_err(|e: reqwest::Error| e.to_string()),
            };

            AppEvent::PostsFetched { request_id, result }
        });
    }

    fn request_more_posts(&mut self) {
        if self.loading || !self.has_more_posts {
            return;
        }

        if self.next_story_index >= self.story_ids.len() {
            self.has_more_posts = false;
            return;
        }

        let (request_id, cancel_token) = self.begin_posts_request();

        let start = self.next_story_index;
        let next_story_index = start
            .saturating_add(POSTS_PAGE_SIZE)
            .min(self.story_ids.len());
        let page_ids: Vec<u64> = self.story_ids[start..next_story_index].to_vec();
        let feed = self.selected_feed.api_feed();
        let client = self.hn_client.clone();

        self.events.send_async(async move {
            let result: Result<PostsFetchResult, String> = tokio::select! {
                _ = cancel_token.cancelled() => Err("Cancelled".to_string()),
                result = client.fetch_items_by_ids(&page_ids, feed) => {
                    result
                        .map(|items| PostsFetchResult {
                            mode: PostsFetchMode::Append,
                            story_ids: None,
                            items,
                            next_story_index,
                        })
                        .map_err(|e| e.to_string())
                },
            };

            AppEvent::PostsFetched { request_id, result }
        });
    }

    fn handle_posts_fetched(&mut self, request_id: u64, result: Result<PostsFetchResult, String>) {
        if self.active_posts_request_id != Some(request_id) {
            return;
        }

        self.loading = false;
        self.active_posts_request_id = None;
        self.posts_request_cancel = None;

        match result {
            Ok(payload) => {
                self.posts_notice = None;

                if let Some(story_ids) = payload.story_ids {
                    self.story_ids = story_ids;
                }

                self.next_story_index = payload.next_story_index;
                let incoming_posts = Self::posts_from_items(payload.items);

                match payload.mode {
                    PostsFetchMode::Replace => {
                        self.posts = incoming_posts;
                    }
                    PostsFetchMode::Append => {
                        self.posts.extend(incoming_posts);
                    }
                }
                self.last_fetched = Some(Self::current_hhmm());

                self.has_more_posts = self.next_story_index < self.story_ids.len();

                if self.posts.is_empty() {
                    self.list_state.select(None);
                } else {
                    let selected = self.list_state.selected().unwrap_or(0);
                    let max_index = self.posts.len().saturating_sub(1);
                    self.list_state.select(Some(selected.min(max_index)));
                }

                self.cache_current_feed();
            }
            Err(err) => {
                if err == "Cancelled" {
                    return;
                }
                if self.posts.is_empty() {
                    self.posts_notice = Some(format!("Failed to load posts: {err}"));
                }
            }
        }
    }

    fn has_reached_load_more_threshold(&self) -> bool {
        let len = self.posts.len();
        if len == 0 {
            return self.has_more_posts;
        }

        let Some(selected_index) = self.list_state.selected() else {
            return false;
        };

        let threshold_count = len
            .saturating_mul(LOAD_MORE_TRIGGER_NUMERATOR)
            .saturating_add(LOAD_MORE_TRIGGER_DENOMINATOR - 1)
            / LOAD_MORE_TRIGGER_DENOMINATOR;

        selected_index.saturating_add(1) >= threshold_count.max(1)
    }

    fn load_more_posts(&mut self) {
        if self.loading || self.comments_open || !self.has_more_posts {
            return;
        }

        if !self.has_reached_load_more_threshold() {
            return;
        }

        self.request_more_posts();
    }

    fn posts_from_items(items: Vec<Item>) -> Vec<Post> {
        items
            .into_iter()
            .filter_map(|item| {
                if item.dead || item.deleted {
                    return None;
                }

                let post_type = PostType::from_kind(item.kind.as_deref())?;
                let title = item.title?;
                let url = item.url?;

                Some(Post {
                    id: item.id,
                    title,
                    url,
                    post_type,
                    points: item.score.unwrap_or_default(),
                    comments: item.descendants.unwrap_or_default(),
                    author: item
                        .by
                        .filter(|author| !author.is_empty())
                        .unwrap_or_else(|| "unknown".to_string()),
                    published_at: item.time.unwrap_or_default(),
                })
            })
            .collect()
    }

    fn cache_current_feed(&mut self) {
        self.feed_cache.insert(
            self.selected_feed,
            CachedFeed {
                story_ids: self.story_ids.clone(),
                next_story_index: self.next_story_index,
                has_more_posts: self.has_more_posts,
                posts: self.posts.clone(),
                selected_index: self.list_state.selected(),
                last_fetched: self.last_fetched.clone(),
            },
        );
    }

    fn restore_feed_from_cache(&mut self, feed: FeedTab) -> bool {
        let Some(cached) = self.feed_cache.get(&feed).cloned() else {
            return false;
        };

        self.story_ids = cached.story_ids;
        self.next_story_index = cached.next_story_index;
        self.has_more_posts = cached.has_more_posts;
        self.posts = cached.posts;
        self.last_fetched = cached.last_fetched;
        self.posts_notice = None;

        if self.posts.is_empty() {
            self.list_state.select(None);
        } else {
            let selected = cached
                .selected_index
                .unwrap_or(0)
                .min(self.posts.len().saturating_sub(1));
            self.list_state.select(Some(selected));
        }

        true
    }

    fn clear_feed_state(&mut self) {
        self.story_ids.clear();
        self.next_story_index = 0;
        self.has_more_posts = true;
        self.posts.clear();
        self.posts_notice = None;
        self.last_fetched = None;
        self.list_state.select(None);
    }

    fn select_next_feed(&mut self) {
        self.switch_feed(1);
    }

    fn select_previous_feed(&mut self) {
        self.switch_feed(-1);
    }

    fn switch_feed(&mut self, delta: isize) {
        let count = FeedTab::ALL.len() as isize;
        let current = self.selected_feed.index() as isize;
        let next_index = (current + delta + count) % count;
        let next_feed = FeedTab::from_index(next_index as usize);
        self.switch_to_feed(next_feed);
    }

    fn switch_to_feed(&mut self, next_feed: FeedTab) {
        if next_feed == self.selected_feed {
            return;
        }

        self.cache_current_feed();
        self.selected_feed = next_feed;
        if !self.restore_feed_from_cache(next_feed) {
            self.clear_feed_state();
        }
        self.events.send(AppEvent::Refresh);
    }

    fn select_next(&mut self) {
        let len = self.posts.len();
        if len == 0 {
            self.list_state.select(None);
            return;
        }

        let next = match self.list_state.selected() {
            Some(i) if i + 1 < len => i + 1,
            _ => 0,
        };

        self.list_state.select(Some(next));
    }

    fn select_previous(&mut self) {
        let len = self.posts.len();
        if len == 0 {
            self.list_state.select(None);
            return;
        }

        let prev = match self.list_state.selected() {
            Some(0) | None => len - 1,
            Some(i) => i - 1,
        };

        self.list_state.select(Some(prev));
    }

    fn selected_post(&self) -> Option<&Post> {
        let index = self.list_state.selected()?;
        self.posts.get(index)
    }

    fn post_by_id(&self, post_id: u64) -> Option<&Post> {
        self.posts.iter().find(|post| post.id == post_id)
    }

    fn comments_post(&self) -> Option<&Post> {
        self.comments_for_post_id
            .and_then(|post_id| self.post_by_id(post_id))
    }

    fn bookmark_post(&mut self, post: Post) {
        if self.bookmarks.iter().any(|bookmark| bookmark.id == post.id) {
            return;
        }

        let was_empty = self.bookmarks.is_empty();
        self.bookmarks.push(post);
        if was_empty {
            self.bookmarks_collapsed = true;
        }
        self.ensure_bookmarks_selection();
    }

    fn bookmark_selected_post(&mut self) {
        let Some(post) = self.selected_post().cloned() else {
            return;
        };
        self.bookmark_post(post);
    }

    fn bookmark_comments_post(&mut self) {
        let Some(post) = self.comments_post().cloned() else {
            return;
        };
        self.bookmark_post(post);
    }

    fn selected_bookmark(&self) -> Option<&Post> {
        let index = self.bookmarks_state.selected()?;
        self.bookmarks.get(index)
    }

    fn open_selected_bookmark(&mut self) {
        let Some(bookmark) = self.selected_bookmark() else {
            return;
        };
        self.events.send(AppEvent::OpenPost(bookmark.url.clone()));
    }

    fn open_all_bookmarks(&mut self) {
        if self.bookmarks.is_empty() {
            return;
        }

        for bookmark in &self.bookmarks {
            self.events.send(AppEvent::OpenPost(bookmark.url.clone()));
        }
    }

    fn select_post_from_bookmark(&mut self) {
        let Some(bookmark_id) = self.selected_bookmark().map(|bookmark| bookmark.id) else {
            return;
        };
        let Some(post_index) = self.posts.iter().position(|post| post.id == bookmark_id) else {
            return;
        };

        self.list_state.select(Some(post_index));
        self.open_comments_for_selected();
    }

    fn open_selected_post(&mut self) {
        let Some(url) = self.selected_post().map(|post| post.url.clone()) else {
            return;
        };
        self.events.send(AppEvent::OpenPost(url));
    }

    fn open_comments_post(&mut self) {
        let Some(url) = self.comments_post().map(|post| post.url.clone()) else {
            return;
        };
        self.events.send(AppEvent::OpenPost(url));
    }

    fn open_comments_for_selected(&mut self) {
        let Some((post_id, post_type)) = self.selected_post().map(|post| (post.id, post.post_type))
        else {
            return;
        };

        self.set_focus_pane(Pane::Comments);
        self.comments_open = true;
        self.comments_scroll = 0;
        self.comments_viewport_height = 0;
        self.comment_line_count = 0;
        self.comment_start_lines.clear();
        self.load_comments(post_id, post_type);
    }

    fn close_comments_view(&mut self) {
        self.set_focus_pane(Pane::Posts);
        self.reset_comments_state();
    }

    fn reset_comments_state(&mut self) {
        self.comments_open = false;
        self.comments.clear();
        self.comments_for_post_id = None;
        self.comments_loading = false;
        self.comments_error = None;
        self.comments_notice = None;
        self.comments_scroll = 0;
        self.comments_viewport_height = 0;
        self.comment_line_count = 0;
        self.comment_start_lines.clear();
    }

    fn load_comments(&mut self, post_id: u64, post_type: PostType) {
        self.comments_for_post_id = Some(post_id);
        self.comments_error = None;
        self.comments_notice = None;
        self.comments_loading = false;
        self.comment_start_lines.clear();

        if post_type == PostType::Job {
            self.comments.clear();
            self.comments_notice = Some("Jobs do not have comment threads.".to_string());
            return;
        }

        let should_refresh = if let Some(cached) = self.comments_cache.get(&post_id) {
            self.comments = cached.comments.clone();
            cached.fetched_at.elapsed() >= Duration::from_secs(COMMENTS_CACHE_REFRESH_AFTER_SECS)
        } else {
            self.comments.clear();
            true
        };

        if !should_refresh {
            return;
        }

        self.comments_loading = true;

        let client = self.hn_client.clone();
        self.events.send_async(async move {
            let result = client
                .fetch_comments(post_id, 75)
                .await
                .map_err(|e| e.to_string());
            AppEvent::LoadCommentsComplete { post_id, result }
        });
    }

    fn max_comment_scroll(&self) -> u16 {
        self.comment_line_count
            .saturating_sub(self.comments_viewport_height) as u16
    }

    fn comment_page_step(&self) -> u16 {
        self.comments_viewport_height.saturating_sub(1).max(1) as u16
    }

    fn clamp_comments_scroll(&mut self) {
        let max_scroll = self.max_comment_scroll();
        if self.comments_scroll > max_scroll {
            self.comments_scroll = max_scroll;
        }
    }

    fn scroll_comments_up(&mut self, amount: u16) {
        self.comments_scroll = self.comments_scroll.saturating_sub(amount);
    }

    fn scroll_comments_down(&mut self, amount: u16) {
        let max_scroll = self.max_comment_scroll();
        self.comments_scroll = self.comments_scroll.saturating_add(amount).min(max_scroll);
    }

    fn jump_to_next_sibling_comment(&mut self) {
        let Some(current_index) = current_index_from_scroll(
            &self.comment_start_lines,
            self.comments.len(),
            self.comments_scroll,
        ) else {
            return;
        };

        if let Some(next_index) = next_sibling_or_outer_index(&self.comments, current_index) {
            self.jump_to_comment(next_index);
        }
    }

    fn jump_to_previous_sibling_comment(&mut self) {
        let Some(current_index) = current_index_from_scroll(
            &self.comment_start_lines,
            self.comments.len(),
            self.comments_scroll,
        ) else {
            return;
        };

        if let Some(parent_index) = previous_sibling_or_parent_index(&self.comments, current_index)
        {
            self.jump_to_comment(parent_index);
        }
    }

    fn jump_to_next_comment(&mut self) {
        let Some(current_index) = current_index_from_scroll(
            &self.comment_start_lines,
            self.comments.len(),
            self.comments_scroll,
        ) else {
            return;
        };

        if let Some(next_index) = next_comment_index(self.comments.len(), current_index) {
            self.jump_to_comment(next_index);
        }
    }

    fn jump_to_previous_comment(&mut self) {
        let Some(current_index) = current_index_from_scroll(
            &self.comment_start_lines,
            self.comments.len(),
            self.comments_scroll,
        ) else {
            return;
        };

        if let Some(prev_index) = previous_comment_index(current_index) {
            self.jump_to_comment(prev_index);
        }
    }

    fn jump_to_comment(&mut self, index: usize) {
        let Some(line) = self.comment_start_lines.get(index) else {
            return;
        };

        self.comments_scroll = (*line).min(self.max_comment_scroll());
    }

    fn bookmarks_visible(&self) -> bool {
        !self.bookmarks.is_empty()
    }

    fn bookmarks_width_percent(&self) -> u16 {
        if self.bookmarks_collapsed {
            if self.comments_open { 10 } else { 12 }
        } else if self.comments_open {
            20
        } else {
            30
        }
    }

    fn close_bookmarks_pane(&mut self) {
        if self.bookmarks.is_empty() {
            self.set_focus_pane(Pane::Posts);
            return;
        }
        self.bookmarks_collapsed = true;
        self.set_focus_pane(Pane::Posts);
    }

    fn open_bookmarks_pane(&mut self) {
        if self.bookmarks.is_empty() {
            return;
        }
        self.bookmarks_collapsed = false;
        self.set_focus_pane(Pane::Bookmarks);
        self.ensure_bookmarks_selection();
    }

    fn focus_next_pane(&mut self) {
        self.cycle_focus(1);
    }

    fn focus_previous_pane(&mut self) {
        self.cycle_focus(-1);
    }

    fn cycle_focus(&mut self, delta: isize) {
        let panes = self.visible_panes();
        if panes.len() <= 1 {
            return;
        }

        let current_index = panes
            .iter()
            .position(|pane| *pane == self.focus_pane)
            .unwrap_or(0) as isize;
        let pane_count = panes.len() as isize;
        let next_index = (current_index + delta + pane_count) % pane_count;
        self.set_focus_pane(panes[next_index as usize]);
    }

    fn visible_panes(&self) -> Vec<Pane> {
        let mut panes = Vec::with_capacity(4);
        panes.push(Pane::Feeds);
        if self.bookmarks_visible() {
            panes.push(Pane::Bookmarks);
        }
        panes.push(Pane::Posts);
        if self.comments_open {
            panes.push(Pane::Comments);
        }
        panes
    }

    fn ensure_focus_valid(&mut self) {
        let panes = self.visible_panes();
        if panes.is_empty() {
            self.focus_pane = Pane::Posts;
            return;
        }

        if !panes.contains(&self.focus_pane) {
            self.focus_pane = Pane::Posts;
        }

        self.ensure_bookmarks_selection();
    }

    fn ensure_bookmarks_selection(&mut self) {
        if self.bookmarks.is_empty() {
            self.bookmarks_state.select(None);
            self.bookmarks_collapsed = false;
            if self.focus_pane == Pane::Bookmarks {
                self.focus_pane = Pane::Posts;
            }
            return;
        }

        let max_index = self.bookmarks.len().saturating_sub(1);
        let selected = self.bookmarks_state.selected().unwrap_or(0).min(max_index);
        self.bookmarks_state.select(Some(selected));
    }

    fn select_next_bookmark(&mut self) {
        if self.bookmarks.is_empty() {
            self.bookmarks_state.select(None);
            return;
        }

        let len = self.bookmarks.len();
        let next = match self.bookmarks_state.selected() {
            Some(i) if i + 1 < len => i + 1,
            _ => 0,
        };
        self.bookmarks_state.select(Some(next));
    }

    fn select_previous_bookmark(&mut self) {
        if self.bookmarks.is_empty() {
            self.bookmarks_state.select(None);
            return;
        }

        let len = self.bookmarks.len();
        let prev = match self.bookmarks_state.selected() {
            Some(0) | None => len - 1,
            Some(i) => i - 1,
        };
        self.bookmarks_state.select(Some(prev));
    }

    fn remove_selected_bookmark(&mut self) {
        let Some(selected) = self.bookmarks_state.selected() else {
            return;
        };

        if selected >= self.bookmarks.len() {
            self.ensure_bookmarks_selection();
            return;
        }

        self.bookmarks.remove(selected);

        if self.bookmarks.is_empty() {
            self.bookmarks_state.select(None);
            self.bookmarks_collapsed = false;
            if self.focus_pane == Pane::Bookmarks {
                self.focus_pane = Pane::Posts;
            }
            return;
        }

        let next_selected = selected.min(self.bookmarks.len().saturating_sub(1));
        self.bookmarks_state.select(Some(next_selected));
    }

    fn set_focus_pane(&mut self, pane: Pane) {
        if self.focus_pane == Pane::Bookmarks && pane != Pane::Bookmarks && self.bookmarks_visible()
        {
            self.bookmarks_collapsed = true;
        }
        self.focus_pane = pane;
        if pane == Pane::Bookmarks && self.bookmarks_visible() {
            self.bookmarks_collapsed = false;
            self.ensure_bookmarks_selection();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn base_item(id: u64) -> Item {
        Item {
            id,
            title: None,
            url: None,
            score: None,
            descendants: None,
            by: None,
            time: None,
            text: None,
            kids: None,
            kind: None,
            dead: false,
            deleted: false,
        }
    }

    #[test]
    fn posts_from_items_filters_invalid_and_maps_defaults() {
        let mut story = base_item(1);
        story.title = Some("Story title".to_string());
        story.url = Some("https://example.com/story".to_string());
        story.kind = Some("story".to_string());
        story.score = Some(123);
        story.descendants = Some(45);
        story.by = Some("alice".to_string());
        story.time = Some(1_700_000_000);

        let mut job = base_item(2);
        job.title = Some("Job title".to_string());
        job.url = Some("https://example.com/job".to_string());
        job.kind = Some("job".to_string());
        job.by = Some(String::new());

        let mut comment_kind = base_item(3);
        comment_kind.title = Some("Comment-like post".to_string());
        comment_kind.url = Some("https://example.com/comment".to_string());
        comment_kind.kind = Some("comment".to_string());

        let mut dead_story = base_item(4);
        dead_story.title = Some("Dead".to_string());
        dead_story.url = Some("https://example.com/dead".to_string());
        dead_story.kind = Some("story".to_string());
        dead_story.dead = true;

        let mut missing_title = base_item(5);
        missing_title.url = Some("https://example.com/missing-title".to_string());
        missing_title.kind = Some("story".to_string());

        let posts =
            App::posts_from_items(vec![story, job, comment_kind, dead_story, missing_title]);

        assert_eq!(posts.len(), 2);

        assert_eq!(posts[0].id, 1);
        assert_eq!(posts[0].title, "Story title");
        assert_eq!(posts[0].url, "https://example.com/story");
        assert!(matches!(posts[0].post_type, PostType::Story));
        assert_eq!(posts[0].points, 123);
        assert_eq!(posts[0].comments, 45);
        assert_eq!(posts[0].author, "alice");
        assert_eq!(posts[0].published_at, 1_700_000_000);

        assert_eq!(posts[1].id, 2);
        assert_eq!(posts[1].title, "Job title");
        assert_eq!(posts[1].url, "https://example.com/job");
        assert!(matches!(posts[1].post_type, PostType::Job));
        assert_eq!(posts[1].points, 0);
        assert_eq!(posts[1].comments, 0);
        assert_eq!(posts[1].author, "unknown");
        assert_eq!(posts[1].published_at, 0);
    }

    fn sample_post(id: u64, title: &str) -> Post {
        Post {
            id,
            title: title.to_string(),
            url: format!("https://example.com/{id}"),
            post_type: PostType::Story,
            points: 0,
            comments: 0,
            author: "author".to_string(),
            published_at: 0,
        }
    }

    fn sample_comment(author: &str, text: &str) -> Comment {
        Comment {
            author: author.to_string(),
            text: text.to_string(),
            published_at: 0,
            depth: 0,
            ancestor_has_next_sibling: Vec::new(),
            is_last_sibling: true,
        }
    }

    #[tokio::test]
    async fn bookmark_selected_post_adds_once_per_post_id() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first"), sample_post(2, "second")];
        app.list_state.select(Some(0));

        app.bookmark_selected_post();
        app.bookmark_selected_post();
        app.list_state.select(Some(1));
        app.bookmark_selected_post();

        assert_eq!(app.bookmarks.len(), 2);
        assert_eq!(app.bookmarks[0].id, 1);
        assert_eq!(app.bookmarks[1].id, 2);
    }

    #[tokio::test]
    async fn bookmarks_are_hidden_until_first_bookmark() {
        let mut app = App::new();
        assert!(!app.bookmarks_visible());

        app.posts = vec![sample_post(1, "first")];
        app.list_state.select(Some(0));
        app.bookmark_selected_post();

        assert!(app.bookmarks_visible());
        assert!(app.bookmarks_collapsed);
    }

    #[tokio::test]
    async fn focus_cycles_between_comments_posts_bookmarks_and_feeds() {
        let mut app = App::new();
        app.comments_open = true;
        app.focus_pane = Pane::Comments;
        app.posts = vec![sample_post(1, "first")];
        app.list_state.select(Some(0));
        app.bookmark_selected_post();

        app.focus_previous_pane();
        assert_eq!(app.focus_pane, Pane::Posts);

        app.focus_previous_pane();
        assert_eq!(app.focus_pane, Pane::Bookmarks);

        app.focus_previous_pane();
        assert_eq!(app.focus_pane, Pane::Feeds);
    }

    #[tokio::test]
    async fn posts_pane_supports_vim_style_jk_navigation() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first"), sample_post(2, "second")];
        app.list_state.select(Some(0));

        app.handle_posts_key(KeyCode::Char('j'));
        assert_eq!(app.list_state.selected(), Some(1));

        app.handle_posts_key(KeyCode::Char('k'));
        assert_eq!(app.list_state.selected(), Some(0));
    }

    #[tokio::test]
    async fn comments_pane_bookmarks_the_post_being_viewed() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first"), sample_post(2, "second")];
        app.comments_open = true;
        app.comments_for_post_id = Some(1);
        app.focus_pane = Pane::Comments;
        app.list_state.select(Some(1));

        app.handle_comments_key(KeyCode::Char('b'));

        assert_eq!(app.bookmarks.len(), 1);
        assert_eq!(app.bookmarks[0].id, 1);
    }

    #[tokio::test]
    async fn opening_comments_uses_fresh_cache_without_fetch() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first")];
        app.list_state.select(Some(0));
        app.comments_cache.insert(
            1,
            CachedComments {
                comments: vec![sample_comment("alice", "cached")],
                fetched_at: Instant::now(),
            },
        );

        app.open_comments_for_selected();

        assert!(app.comments_open);
        assert_eq!(app.comments_for_post_id, Some(1));
        assert!(!app.comments_loading);
        assert_eq!(app.comments.len(), 1);
        assert_eq!(app.comments[0].text, "cached");
    }

    #[tokio::test]
    async fn opening_comments_with_stale_cache_keeps_comments_and_refreshes() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first")];
        app.list_state.select(Some(0));
        app.comments_cache.insert(
            1,
            CachedComments {
                comments: vec![sample_comment("alice", "cached")],
                fetched_at: Instant::now()
                    - Duration::from_secs(COMMENTS_CACHE_REFRESH_AFTER_SECS + 1),
            },
        );

        app.open_comments_for_selected();

        assert!(app.comments_open);
        assert_eq!(app.comments_for_post_id, Some(1));
        assert!(app.comments_loading);
        assert_eq!(app.comments.len(), 1);
        assert_eq!(app.comments[0].text, "cached");
    }

    #[tokio::test]
    async fn failed_comments_refresh_keeps_cached_comments_visible() {
        let mut app = App::new();
        app.comments_open = true;
        app.comments_for_post_id = Some(1);
        app.comments_loading = true;
        app.comments = vec![sample_comment("alice", "cached")];
        app.comments_cache.insert(
            1,
            CachedComments {
                comments: vec![sample_comment("alice", "cached")],
                fetched_at: Instant::now()
                    - Duration::from_secs(COMMENTS_CACHE_REFRESH_AFTER_SECS + 1),
            },
        );

        app.handle_app_event(AppEvent::LoadCommentsComplete {
            post_id: 1,
            result: Err("network down".to_string()),
        });

        assert!(!app.comments_loading);
        assert_eq!(app.comments.len(), 1);
        assert_eq!(app.comments[0].text, "cached");
        assert!(app.comments_error.is_none());
    }

    #[tokio::test]
    async fn feed_switching_requires_feeds_focus() {
        let mut app = App::new();
        assert_eq!(app.selected_feed, FeedTab::Top);

        app.handle_posts_key(KeyCode::Right);
        assert_eq!(app.selected_feed, FeedTab::Top);

        app.handle_feeds_key(KeyCode::Right);
        assert_eq!(app.selected_feed, FeedTab::New);

        app.handle_feeds_key(KeyCode::Left);
        assert_eq!(app.selected_feed, FeedTab::Top);
    }

    #[tokio::test]
    async fn enter_in_feeds_pane_moves_focus_to_posts() {
        let mut app = App::new();
        app.focus_pane = Pane::Feeds;

        app.handle_feeds_key(KeyCode::Enter);

        assert_eq!(app.focus_pane, Pane::Posts);
    }

    #[tokio::test]
    async fn pane_shortcuts_focus_panes() {
        let mut app = App::new();
        app.focus_pane = Pane::Feeds;
        app.comments_open = true;

        app.handle_key_event(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
            .expect("pane key should be handled");
        assert_eq!(app.focus_pane, Pane::Posts);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE))
            .expect("pane key should be handled");
        assert_eq!(app.focus_pane, Pane::Feeds);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE))
            .expect("pane key should be handled");
        assert_eq!(app.focus_pane, Pane::Comments);
    }

    #[tokio::test]
    async fn feed_switch_refresh_keeps_feeds_focus() {
        let mut app = App::new();
        app.focus_pane = Pane::Feeds;

        app.handle_feeds_key(KeyCode::Right);
        app.handle_app_event(AppEvent::Refresh);

        assert_eq!(app.selected_feed, FeedTab::New);
        assert_eq!(app.focus_pane, Pane::Feeds);
    }

    #[tokio::test]
    async fn key_one_toggles_bookmarks_when_focused() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first")];
        app.list_state.select(Some(0));
        app.bookmark_selected_post();
        app.focus_pane = Pane::Bookmarks;
        app.bookmarks_collapsed = false;

        app.handle_key_event(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .expect("pane key should be handled");
        assert!(app.bookmarks_collapsed);
        assert_eq!(app.focus_pane, Pane::Bookmarks);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE))
            .expect("pane key should be handled");
        assert!(!app.bookmarks_collapsed);
        assert_eq!(app.focus_pane, Pane::Bookmarks);
    }

    #[tokio::test]
    async fn navigating_away_from_bookmarks_collapses_it() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first")];
        app.list_state.select(Some(0));
        app.bookmark_selected_post();
        app.open_bookmarks_pane();

        assert_eq!(app.focus_pane, Pane::Bookmarks);
        assert!(!app.bookmarks_collapsed);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
            .expect("pane key should be handled");

        assert_eq!(app.focus_pane, Pane::Posts);
        assert!(app.bookmarks_collapsed);
    }

    #[tokio::test]
    async fn tab_focus_to_bookmarks_expands_it() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first")];
        app.list_state.select(Some(0));
        app.bookmark_selected_post();
        app.focus_pane = Pane::Posts;
        app.bookmarks_collapsed = true;

        app.handle_key_event(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
            .expect("pane key should be handled");

        assert_eq!(app.focus_pane, Pane::Bookmarks);
        assert!(!app.bookmarks_collapsed);
    }

    #[tokio::test]
    async fn deleting_bookmarks_updates_focus_and_selection() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first"), sample_post(2, "second")];
        app.list_state.select(Some(0));
        app.bookmark_selected_post();
        app.list_state.select(Some(1));
        app.bookmark_selected_post();
        app.focus_pane = Pane::Bookmarks;
        app.bookmarks_collapsed = false;
        app.bookmarks_state.select(Some(0));

        app.handle_bookmarks_key(KeyCode::Char('d'));
        assert_eq!(app.bookmarks.len(), 1);
        assert_eq!(app.bookmarks[0].id, 2);
        assert_eq!(app.bookmarks_state.selected(), Some(0));
        assert_eq!(app.focus_pane, Pane::Bookmarks);

        app.handle_bookmarks_key(KeyCode::Char('d'));
        assert!(app.bookmarks.is_empty());
        assert!(!app.bookmarks_visible());
        assert_eq!(app.focus_pane, Pane::Posts);
    }

    #[tokio::test]
    async fn opening_all_bookmarks_keeps_bookmark_state() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first"), sample_post(2, "second")];
        app.list_state.select(Some(0));
        app.bookmark_selected_post();
        app.list_state.select(Some(1));
        app.bookmark_selected_post();
        app.focus_pane = Pane::Bookmarks;
        app.bookmarks_collapsed = false;
        app.bookmarks_state.select(Some(1));

        app.handle_bookmarks_key(KeyCode::Char('a'));

        assert_eq!(app.bookmarks.len(), 2);
        assert_eq!(app.bookmarks_state.selected(), Some(1));
        assert_eq!(app.focus_pane, Pane::Bookmarks);
        assert!(!app.bookmarks_collapsed);
    }

    #[tokio::test]
    async fn enter_from_bookmarks_opens_comments_for_selected_post() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first")];
        app.list_state.select(Some(0));
        app.bookmark_selected_post();
        app.focus_pane = Pane::Bookmarks;
        app.bookmarks_collapsed = false;
        app.bookmarks_state.select(Some(0));

        app.handle_bookmarks_key(KeyCode::Enter);
        assert!(app.comments_open);
        assert_eq!(app.comments_for_post_id, Some(1));
        assert_eq!(app.focus_pane, Pane::Comments);
    }

    #[tokio::test]
    async fn esc_in_bookmarks_collapses_pane_but_keeps_it_visible() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first")];
        app.list_state.select(Some(0));
        app.bookmark_selected_post();
        app.focus_pane = Pane::Bookmarks;

        app.handle_bookmarks_key(KeyCode::Esc);

        assert!(app.bookmarks_visible());
        assert!(app.bookmarks_collapsed);
        assert_eq!(app.focus_pane, Pane::Posts);
    }

    #[tokio::test]
    async fn enter_expands_collapsed_bookmarks_pane() {
        let mut app = App::new();
        app.posts = vec![sample_post(1, "first")];
        app.list_state.select(Some(0));
        app.bookmark_selected_post();
        app.bookmarks_collapsed = true;
        app.focus_pane = Pane::Bookmarks;

        app.handle_bookmarks_key(KeyCode::Enter);

        assert!(!app.bookmarks_collapsed);
        assert_eq!(app.focus_pane, Pane::Bookmarks);
    }
}
