use futures::{StreamExt, stream};
use reqwest::Error;
use serde::Deserialize;
use std::{result::Result, time::Duration};

const TOP_STORIES_URL: &str = "https://hacker-news.firebaseio.com/v0/topstories.json";
const ITEM_URL_BASE: &str = "https://hacker-news.firebaseio.com/v0/item";
const HN_DISCUSSION_URL_BASE: &str = "https://news.ycombinator.com/item?id=";
const DEFAULT_CONCURRENCY: usize = 20;
const DEFAULT_TIMEOUT_SECS: u64 = 10;
const USER_AGENT: &str = "lazynews/0.1";

#[derive(Debug, Deserialize)]
pub struct Item {
    pub id: u64,
    pub title: Option<String>,
    pub url: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub dead: bool,
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Clone)]
pub struct HackerNewsApi {
    client: reqwest::Client,
}

impl HackerNewsApi {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { client }
    }

    async fn fetch_single_item(&self, id: u64) -> Result<Item, Error> {
        let item_url = format!("{ITEM_URL_BASE}/{id}.json");
        self.client
            .get(item_url)
            .send()
            .await?
            .error_for_status()?
            .json::<Item>()
            .await
    }

    pub async fn fetch_items(&self, limit: usize) -> Result<Vec<Item>, Error> {
        let ids = self
            .client
            .get(TOP_STORIES_URL)
            .send()
            .await?
            .error_for_status()?
            .json::<Vec<u64>>()
            .await?;

        let mut indexed: Vec<(usize, Item)> = stream::iter(ids.into_iter().take(limit).enumerate())
            .map(|(idx, id)| async move {
                self.fetch_single_item(id)
                    .await
                    .map(|item| (idx, item))
                    .ok()
            })
            .buffer_unordered(DEFAULT_CONCURRENCY)
            .filter_map(|item| async move { item })
            .collect()
            .await;

        indexed.sort_by_key(|(idx, _)| *idx);

        Ok(indexed
            .into_iter()
            .map(|(_, mut item)| {
                if item.url.is_none() {
                    item.url = Some(format!("{HN_DISCUSSION_URL_BASE}{}", item.id));
                }
                item
            })
            .filter(|item| !item.dead && !item.deleted && item.kind.as_deref() == Some("story"))
            .collect())
    }
}
