use crate::{
    event::{AppEvent, Event, EventHandler},
    hn::{Comment, HackerNewsApi},
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

pub struct App {
    running: bool,
    hn_client: HackerNewsApi,
    events: EventHandler,
    loading_frame: usize,
    posts: Vec<Post>,
    status: Option<String>,
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

impl App {
    pub fn new() -> Self {
        Self {
            running: true,
            hn_client: HackerNewsApi::new(),
            events: EventHandler::new(),
            loading_frame: 0,
            posts: Vec::new(),
            status: None,
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

        let block = if self.comments_open {
            Block::bordered()
                .title("Top Stories")
                .border_style(Style::new().fg(POST_META_COLOR))
        } else {
            Block::bordered().title("Top Stories")
        };

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
            .map(|post| format!("{} | {}", post.title, post.comments))
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
                KeyCode::Char('n') | KeyCode::Char('N') => self.jump_to_next_sibling_comment(),
                KeyCode::Up => self.scroll_comments_up(1),
                KeyCode::Down => self.scroll_comments_down(1),
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
            KeyCode::Down => self.select_next(),
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
                self.loading = true;
                self.loading_frame = 0;
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

                let client = self.hn_client.clone();
                self.events.send_async(async move {
                    let result = client.fetch_items(30).await.map_err(|e| e.to_string());
                    AppEvent::RefreshComplete(result)
                });
            }
            AppEvent::RefreshComplete(result) => {
                self.loading = false;
                match result {
                    Ok(items) => {
                        self.posts = items
                            .into_iter()
                            .filter_map(|item| {
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
                            .collect();
                        self.list_state
                            .select(if self.posts.is_empty() { None } else { Some(0) });
                    }
                    Err(err) => {
                        self.status = Some(format!("Refresh failed: {err}"));
                    }
                }
            }
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
            AppEvent::OpenPost(url) => match webbrowser::open(&url) {
                Ok(_) => {
                    self.status = Some(format!("Opened {}", url));
                }
                Err(err) => {
                    self.status = Some(format!("Failed to open URL: {err}"));
                }
            },
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
            if depth < current_depth {
                break;
            }

            if depth == current_depth {
                self.jump_to_comment(next_index);
                return;
            }
        }
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
