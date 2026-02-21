use crate::{
    event::{AppEvent, Event, EventHandler},
    hn::HackerNewsApi,
    ui::{POST_META_COLOR, POST_SELECTED_COLOR, SPINNER_FRAMES, format_age},
};
use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    style::{Style, Stylize},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState},
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
}

#[derive(Debug)]
struct Post {
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

        let mut instruction_spans: Vec<Span> = if self.loading {
            vec![
                "Refreshing ".yellow().bold().into(),
                spinner.yellow().bold().into(),
                " ".into(),
            ]
        } else {
            vec!["Refresh ".into(), "<R>".blue().bold().into(), " ".into()]
        };
        instruction_spans.extend([
            "Move ".into(),
            "<Up/Down>".blue().bold().into(),
            " Open ".into(),
            "<Enter/O>".blue().bold().into(),
            " Quit ".into(),
            "<Q> ".blue().bold().into(),
        ]);
        let instructions = Line::from(instruction_spans);

        let block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered())
            .border_set(border::THICK);

        let items: Vec<ListItem> = if self.posts.is_empty() {
            if self.loading {
                vec![ListItem::new(Line::from(format!("Refreshing {spinner}")))]
            } else {
                vec![ListItem::new(Line::from(""))]
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

        let list = List::new(items).block(block).highlight_symbol("> ");

        frame.render_stateful_widget(list, frame.area(), &mut self.list_state);
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        if matches!(key_event.code, KeyCode::Char('c'))
            && key_event.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.exit();
            return Ok(());
        }

        match key_event.code {
            KeyCode::Char('q') => self.events.send(AppEvent::Quit),
            KeyCode::Char('r') => self.events.send(AppEvent::Refresh),
            KeyCode::Up => self.select_previous(),
            KeyCode::Down => self.select_next(),
            KeyCode::Enter | KeyCode::Char('o') => self.activate_selected(),
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
        if self.loading {
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

    fn activate_selected(&mut self) {
        self.open_selected();
    }

    fn open_selected(&mut self) {
        let Some(index) = self.list_state.selected() else {
            return;
        };

        let Some(post) = self.posts.get(index) else {
            return;
        };

        self.events.send(AppEvent::OpenPost(post.url.clone()));
    }
}
