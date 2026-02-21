use crate::{
    event::{AppEvent, Event, EventHandler},
    hn::HackerNewsApi,
};
use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    style::{Color, Style, Stylize},
    symbols::border,
    text::Line,
    widgets::{Block, List, ListItem, ListState},
};

const SELECTED_ORANGE: Color = Color::Rgb(255, 149, 0);
const REFRESH_LIMIT: usize = 30;

const SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

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
        let title = if let Some(status) = &self.status {
            format!("lazynews | {status}")
        } else {
            "lazynews".into()
        };

        let title = Line::from(title.bold());

        let spinner = self.spinner_frame();

        let instructions = if self.loading {
            Line::from(vec![
                "Refreshing ".yellow().bold(),
                spinner.yellow().bold(),
                " ".into(),
                " Move ".into(),
                "<Up/Down>".blue().bold(),
                " Open ".into(),
                "<Enter/O>".blue().bold(),
                " Quit ".into(),
                "<Q> ".blue().bold(),
            ])
        } else {
            Line::from(vec![
                "Refresh".into(),
                "<R>".blue().bold(),
                " Move ".into(),
                "<Up/Down>".blue().bold(),
                " Open ".into(),
                "<Enter/O>".blue().bold(),
                " Quit ".into(),
                "<Q> ".blue().bold(),
            ])
        };

        let block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered())
            .border_set(border::THICK);

        let items: Vec<ListItem> = if self.posts.is_empty() {
            vec![ListItem::new(Line::from(format!("{spinner}")))]
        } else {
            self.posts
                .iter()
                .enumerate()
                .map(|(i, post)| ListItem::new(Line::from(format!("{}. {}", i + 1, post.title))))
                .collect()
        };

        let list = List::new(items)
            .block(block)
            .highlight_symbol("> ")
            .highlight_style(Style::new().fg(SELECTED_ORANGE).bold());

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
                    let result = client
                        .fetch_items(REFRESH_LIMIT)
                        .await
                        .map_err(|e| e.to_string());
                    AppEvent::RefreshComplete(result)
                });
            }
            AppEvent::RefreshComplete(result) => {
                self.loading = false;
                match result {
                    Ok(items) => {
                        self.posts = items
                            .into_iter()
                            .filter_map(|item| match (item.title, item.url) {
                                (Some(title), Some(url)) => Some(Post { title, url }),
                                _ => None,
                            })
                            .collect();
                        self.list_state
                            .select(if self.posts.is_empty() { None } else { Some(0) });
                        self.status = Some(format!("Loaded {} posts", self.posts.len()));
                    }
                    Err(err) => {
                        self.status = Some(format!("Refresh failed: {err}"));
                    }
                }
            }
            AppEvent::OpenPost(url) => todo!(),
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
        let Some(index) = self.list_state.selected() else {
            return;
        };

        if !self.posts.is_empty() && index == self.posts.len() {
            self.load_more();
            return;
        }

        self.open_selected();
    }

    fn open_selected(&mut self) {
        let Some(index) = self.list_state.selected() else {
            return;
        };

        let Some(post) = self.posts.get(index) else {
            return;
        };

        match webbrowser::open(&post.url) {
            Ok(_) => {
                self.status = Some(format!("Opened {}", post.url));
            }
            Err(err) => {
                self.status = Some(format!("Failed to open URL: {err}"));
            }
        }
    }

    fn load_more(&mut self) {
        todo!()
    }
}
