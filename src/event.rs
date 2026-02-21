use crate::hn::Item;
use color_eyre::eyre::OptionExt;
use crossterm::event::{Event as CrosstermEvent, EventStream, KeyEvent, KeyEventKind};
use futures::StreamExt;
use std::{future::Future, result::Result, time::Duration};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

const TICK_FPS: f64 = 30.0;

#[derive(Debug)]
pub enum AppEvent {
    Refresh,
    RefreshComplete(Result<Vec<Item>, String>),
    Quit,
    OpenPost(String),
}

#[derive(Debug)]
pub enum Event {
    Tick,
    Key(KeyEvent),
    App(AppEvent),
}

pub struct EventHandler {
    sender: UnboundedSender<Event>,
    receiver: UnboundedReceiver<Event>,
}

impl EventHandler {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        tokio::spawn(event_task(sender.clone()));
        Self { sender, receiver }
    }

    pub async fn next(&mut self) -> color_eyre::Result<Event> {
        self.receiver
            .recv()
            .await
            .ok_or_eyre("Failed to receive event")
    }

    pub fn send(&self, app_event: AppEvent) {
        let _ = self.sender.send(Event::App(app_event));
    }

    pub fn send_async<F>(&self, task: F)
    where
        F: Future<Output = AppEvent> + Send + 'static,
    {
        let sender = self.sender.clone();
        tokio::spawn(async move {
            let _ = sender.send(Event::App(task.await));
        });
    }
}

async fn event_task(sender: mpsc::UnboundedSender<Event>) {
    let tick_rate = Duration::from_secs_f64(1.0 / TICK_FPS);
    let mut reader = EventStream::new();
    let mut tick = tokio::time::interval(tick_rate);
    loop {
        tokio::select! {
            _ = sender.closed() => {
                break;
            }
            _ = tick.tick() => {
                let _ = sender.send(Event::Tick);
            }
            maybe_event = reader.next() => {
                match maybe_event {
                    Some(Ok(CrosstermEvent::Key(key_event)))
                        if key_event.kind == KeyEventKind::Press =>
                    {
                        let _ = sender.send(Event::Key(key_event));
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => {}
                    None => break,
                }
            }
        };
    }
}
