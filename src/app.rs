use crate::{
    event::{AppEvent, Event, EventHandler, PostsFetchMode, PostsFetchResult},
    hn::{Comment, HackerNewsApi, Item},
    ui::{
        COMMENT_BORDER_COLOR, POST_META_COLOR, POST_SELECTED_COLOR, SPINNER_FRAMES,
        comment_lines as build_comment_lines, format_age, instructions_line,
    },
};
use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    style::{Style, Stylize},
    symbols::border,
    text::Line,
    widgets::{Block, List, ListItem, ListState, Paragraph},
};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct App {
    running: bool,
    hn_client: HackerNewsApi,
    events: EventHandler,
    loading_frame: usize,
    top_story_ids: Vec<u64>,
    next_story_index: usize,
    has_more_posts: bool,
    posts: Vec<Post>,
    last_fetched: Option<String>,
    pub loading: bool,
    list_state: ListState,
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
}

#[derive(Debug)]
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

const POSTS_PAGE_SIZE: usize = 30;
const LOAD_MORE_TRIGGER_NUMERATOR: usize = 3;
const LOAD_MORE_TRIGGER_DENOMINATOR: usize = 4;

impl App {
    pub fn new() -> Self {
        Self {
            running: true,
            hn_client: HackerNewsApi::new(),
            events: EventHandler::new(),
            loading_frame: 0,
            top_story_ids: Vec::new(),
            next_story_index: 0,
            has_more_posts: true,
            posts: Vec::new(),
            last_fetched: None,
            loading: false,
            list_state: ListState::default(),
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
        let title = Line::from("lazynews".bold());
        let spinner = self.spinner_frame();
        let instructions = instructions_line(self.comments_open, self.loading, spinner);

        let outer_block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered())
            .border_set(border::THICK);

        let content_area = outer_block.inner(frame.area());
        frame.render_widget(outer_block, frame.area());

        if self.comments_open {
            let layout =
                Layout::horizontal([Constraint::Percentage(33), Constraint::Percentage(67)]);
            let areas = layout.split(content_area);
            self.render_posts_list(frame, areas[0]);
            self.render_comments_pane(frame, areas[1], spinner);
        } else {
            self.render_posts_list(frame, content_area);
        }
    }

    fn render_posts_list(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let items: Vec<ListItem> = if self.posts.is_empty() {
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

        let mut block = Block::bordered().title("Top Stories");
        if self.comments_open {
            block = block.border_style(Style::new().fg(POST_META_COLOR));
        }
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

    fn render_comments_pane(
        &mut self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        spinner: &str,
    ) {
        let comments_title = self
            .selected_post()
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
                    .title(comments_title)
                    .border_style(Style::new().fg(COMMENT_BORDER_COLOR)),
            )
            .scroll((self.comments_scroll, 0));

        frame.render_widget(widget, area);
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        if matches!(key_event.code, KeyCode::Char('c'))
            && key_event.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.exit();
            return Ok(());
        }

        if self.comments_open {
            match key_event.code {
                KeyCode::Char('q') => self.events.send(AppEvent::Quit),
                KeyCode::Esc => self.close_comments_view(),
                KeyCode::Char('o') => self.open_selected_post(),
                KeyCode::Up => self.jump_to_previous_comment(),
                KeyCode::Down => self.jump_to_next_comment(),
                KeyCode::Left => self.jump_to_previous_sibling_comment(),
                KeyCode::Right => self.jump_to_next_sibling_comment(),
                KeyCode::Char('k') | KeyCode::Char('K') => self.scroll_comments_up(1),
                KeyCode::Char('j') | KeyCode::Char('J') => self.scroll_comments_down(1),
                KeyCode::Home => self.comments_scroll = 0,
                KeyCode::End => self.comments_scroll = self.max_comment_scroll(),
                _ => {}
            }
            return Ok(());
        }

        match key_event.code {
            KeyCode::Char('q') => self.events.send(AppEvent::Quit),
            KeyCode::Char('r') => self.events.send(AppEvent::Refresh),
            KeyCode::Up => self.select_previous(),
            KeyCode::Down => {
                self.select_next();
                self.load_more_posts();
            }
            KeyCode::Enter => self.open_comments_for_selected(),
            KeyCode::Char('o') => self.open_selected_post(),
            _ => {}
        }

        Ok(())
    }

    fn exit(&mut self) {
        self.running = false;
    }

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Quit => self.exit(),
            AppEvent::Refresh => {
                if self.loading {
                    return;
                }
                self.has_more_posts = true;
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
                self.refresh_posts();
            }
            AppEvent::PostsFetched(result) => self.handle_posts_fetched(result),
            AppEvent::LoadCommentsComplete { post_id, result } => {
                if !self.comments_open || self.comments_for_post_id != Some(post_id) {
                    return;
                }

                self.comments_loading = false;
                match result {
                    Ok(comments) => {
                        self.comments = comments;
                        self.comments_error = None;
                        self.comments_notice = None;
                        self.comments_scroll = 0;
                        self.comment_start_lines.clear();
                    }
                    Err(err) => {
                        self.comments.clear();
                        self.comments_error = Some(err);
                        self.comments_notice = None;
                        self.comment_start_lines.clear();
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
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        let minutes = (seconds / 60) % (24 * 60);
        let hour = minutes / 60;
        let minute = minutes % 60;
        format!("{hour:02}:{minute:02}")
    }

    fn refresh_posts(&mut self) {
        self.loading = true;
        self.loading_frame = 0;
        self.top_story_ids.clear();
        self.next_story_index = 0;
        self.posts.clear();
        self.list_state.select(None);

        let client = self.hn_client.clone();
        self.events.send_async(async move {
            let result = async {
                let top_story_ids = client.fetch_top_story_ids().await?;
                let next_story_index = top_story_ids.len().min(POSTS_PAGE_SIZE);
                let page_ids: Vec<u64> = top_story_ids
                    .iter()
                    .take(next_story_index)
                    .copied()
                    .collect();
                let items = client.fetch_items_by_ids(&page_ids).await?;

                Ok(PostsFetchResult {
                    mode: PostsFetchMode::Replace,
                    top_story_ids: Some(top_story_ids),
                    items,
                    next_story_index,
                })
            }
            .await
            .map_err(|e: reqwest::Error| e.to_string());

            AppEvent::PostsFetched(result)
        });
    }

    fn request_more_posts(&mut self) {
        if self.loading || !self.has_more_posts {
            return;
        }

        if self.next_story_index >= self.top_story_ids.len() {
            self.has_more_posts = false;
            return;
        }

        self.loading = true;
        self.loading_frame = 0;

        let start = self.next_story_index;
        let next_story_index = start
            .saturating_add(POSTS_PAGE_SIZE)
            .min(self.top_story_ids.len());
        let page_ids: Vec<u64> = self.top_story_ids[start..next_story_index].to_vec();
        let client = self.hn_client.clone();

        self.events.send_async(async move {
            let result = client
                .fetch_items_by_ids(&page_ids)
                .await
                .map(|items| PostsFetchResult {
                    mode: PostsFetchMode::Append,
                    top_story_ids: None,
                    items,
                    next_story_index,
                })
                .map_err(|e| e.to_string());

            AppEvent::PostsFetched(result)
        });
    }

    fn handle_posts_fetched(&mut self, result: Result<PostsFetchResult, String>) {
        self.loading = false;

        match result {
            Ok(payload) => {
                if let Some(top_story_ids) = payload.top_story_ids {
                    self.top_story_ids = top_story_ids;
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

                self.has_more_posts = self.next_story_index < self.top_story_ids.len();

                if self.posts.is_empty() {
                    self.list_state.select(None);
                } else {
                    let selected = self.list_state.selected().unwrap_or(0);
                    let max_index = self.posts.len().saturating_sub(1);
                    self.list_state.select(Some(selected.min(max_index)));
                }
            }
            Err(_) => {}
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

                let title = item.title?;
                let url = item.url?;
                let post_type = PostType::from_kind(item.kind.as_deref())?;

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

    fn open_selected_post(&mut self) {
        let Some(post) = self.selected_post() else {
            return;
        };

        self.events.send(AppEvent::OpenPost(post.url.clone()));
    }

    fn open_comments_for_selected(&mut self) {
        let Some((post_id, post_type)) = self.selected_post().map(|post| (post.id, post.post_type))
        else {
            return;
        };

        self.comments_open = true;
        self.comments_scroll = 0;
        self.comments_viewport_height = 0;
        self.comment_line_count = 0;
        self.comment_start_lines.clear();
        self.load_comments(post_id, post_type);
    }

    fn close_comments_view(&mut self) {
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
        self.comments.clear();
        self.comments_loading = false;
        self.comment_start_lines.clear();

        if post_type == PostType::Job {
            self.comments_notice = Some("Jobs do not have comment threads.".to_string());
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
        let Some(current_index) = self.current_comment_index_from_scroll() else {
            return;
        };

        let current_depth = self.comments[current_index].depth;

        for next_index in (current_index + 1)..self.comments.len() {
            let depth = self.comments[next_index].depth;
            if depth == current_depth {
                self.jump_to_comment(next_index);
                return;
            }
            if depth < current_depth {
                self.jump_to_comment(next_index);
                return;
            }
        }
    }

    fn jump_to_previous_sibling_comment(&mut self) {
        let Some(current_index) = self.current_comment_index_from_scroll() else {
            return;
        };

        let current_depth = self.comments[current_index].depth;

        for prev_index in (0..current_index).rev() {
            let depth = self.comments[prev_index].depth;
            if depth < current_depth {
                break;
            }

            if depth == current_depth {
                self.jump_to_comment(prev_index);
                return;
            }
        }

        if let Some(parent_index) = self.nearest_parent_comment_index(current_index) {
            self.jump_to_comment(parent_index);
        }
    }

    fn jump_to_next_comment(&mut self) {
        let Some(current_index) = self.current_comment_index_from_scroll() else {
            return;
        };

        if current_index + 1 < self.comments.len() {
            self.jump_to_comment(current_index + 1);
        }
    }

    fn jump_to_previous_comment(&mut self) {
        let Some(current_index) = self.current_comment_index_from_scroll() else {
            return;
        };

        if current_index > 0 {
            self.jump_to_comment(current_index - 1);
        }
    }

    fn nearest_parent_comment_index(&self, current_index: usize) -> Option<usize> {
        let current_depth = self.comments.get(current_index)?.depth;
        if current_depth == 0 {
            return None;
        }

        (0..current_index)
            .rev()
            .find(|&index| self.comments[index].depth < current_depth)
    }

    fn current_comment_index_from_scroll(&self) -> Option<usize> {
        if self.comments.is_empty() || self.comment_start_lines.is_empty() {
            return None;
        }

        let mut current = 0usize;
        for (index, line) in self.comment_start_lines.iter().enumerate() {
            if *line > self.comments_scroll {
                break;
            }
            current = index;
        }

        Some(current)
    }

    fn jump_to_comment(&mut self, index: usize) {
        let Some(line) = self.comment_start_lines.get(index) else {
            return;
        };

        self.comments_scroll = (*line).min(self.max_comment_scroll());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
