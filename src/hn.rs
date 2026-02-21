use futures::{StreamExt, stream};
use reqwest::Error;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    result::Result,
    time::Duration,
};

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
    pub score: Option<u64>,
    pub descendants: Option<u64>,
    pub by: Option<String>,
    pub time: Option<u64>,
    pub text: Option<String>,
    pub kids: Option<Vec<u64>>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub dead: bool,
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Debug)]
pub struct Comment {
    pub author: String,
    pub text: String,
    pub published_at: u64,
    pub depth: usize,
    pub ancestor_has_next_sibling: Vec<bool>,
    pub is_last_sibling: bool,
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
        if limit == 0 {
            return Ok(Vec::new());
        }

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
            .filter(|item| {
                let is_supported = matches!(item.kind.as_deref(), Some("story" | "job"));
                !item.dead && !item.deleted && is_supported
            })
            .take(limit)
            .collect())
    }

    pub async fn fetch_comments(&self, post_id: u64, limit: usize) -> Result<Vec<Comment>, Error> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let post = self.fetch_single_item(post_id).await?;
        let root_kids = post.kids.unwrap_or_default();
        if root_kids.is_empty() {
            return Ok(Vec::new());
        }

        let mut pending: Vec<u64> = root_kids.iter().rev().copied().collect();
        let mut scheduled_ids: HashSet<u64> = root_kids.iter().copied().collect();
        let mut failed_ids: HashSet<u64> = HashSet::new();
        let mut items_by_id: HashMap<u64, Item> = HashMap::new();

        loop {
            if let Some(comments) =
                build_comments_from_cache(&root_kids, limit, &items_by_id, &failed_ids)
                && (comments.len() >= limit || pending.is_empty())
            {
                return Ok(comments);
            }

            if pending.is_empty() {
                break;
            }

            let mut batch: Vec<u64> = Vec::with_capacity(DEFAULT_CONCURRENCY);
            while batch.len() < DEFAULT_CONCURRENCY {
                match pending.pop() {
                    Some(id) => batch.push(id),
                    None => break,
                }
            }

            let mut fetched: Vec<(usize, u64, Option<Item>)> =
                stream::iter(batch.into_iter().enumerate())
                    .map(|(order, id)| async move {
                        let item = self.fetch_single_item(id).await.ok();
                        (order, id, item)
                    })
                    .buffer_unordered(DEFAULT_CONCURRENCY)
                    .collect()
                    .await;

            fetched.sort_by_key(|(order, _, _)| *order);

            for (_, id, maybe_item) in fetched.into_iter().rev() {
                match maybe_item {
                    Some(item) => {
                        for kid in item.kids.as_deref().unwrap_or(&[]).iter().rev() {
                            if scheduled_ids.insert(*kid) {
                                pending.push(*kid);
                            }
                        }
                        items_by_id.insert(id, item);
                    }
                    None => {
                        failed_ids.insert(id);
                    }
                }
            }
        }

        Ok(
            build_comments_from_cache(&root_kids, limit, &items_by_id, &failed_ids)
                .unwrap_or_default(),
        )
    }
}

struct PendingComment {
    id: u64,
    depth: usize,
    ancestor_has_next_sibling: Vec<bool>,
    is_last_sibling: bool,
}

fn build_comments_from_cache(
    root_kids: &[u64],
    limit: usize,
    items_by_id: &HashMap<u64, Item>,
    failed_ids: &HashSet<u64>,
) -> Option<Vec<Comment>> {
    let root_count = root_kids.len();
    let mut stack: Vec<PendingComment> = Vec::with_capacity(root_count);
    for (index, kid) in root_kids.iter().enumerate().rev() {
        stack.push(PendingComment {
            id: *kid,
            depth: 0,
            ancestor_has_next_sibling: Vec::new(),
            is_last_sibling: index + 1 == root_count,
        });
    }

    let mut comments = Vec::new();

    while let Some(node) = stack.pop() {
        if comments.len() >= limit {
            break;
        }

        let Some(item) = items_by_id.get(&node.id) else {
            if failed_ids.contains(&node.id) {
                continue;
            }
            return None;
        };

        let child_ids = item.kids.as_deref().unwrap_or(&[]);
        let child_count = child_ids.len();
        let mut child_ancestor_has_next_sibling = node.ancestor_has_next_sibling.clone();
        child_ancestor_has_next_sibling.push(!node.is_last_sibling);

        for (index, kid) in child_ids.iter().enumerate().rev() {
            stack.push(PendingComment {
                id: *kid,
                depth: node.depth + 1,
                ancestor_has_next_sibling: child_ancestor_has_next_sibling.clone(),
                is_last_sibling: index + 1 == child_count,
            });
        }

        if item.dead || item.deleted {
            continue;
        }

        if !matches!(item.kind.as_deref(), Some("comment")) {
            continue;
        }

        let cleaned_text = clean_comment_text(item.text.as_deref().unwrap_or_default());
        if cleaned_text.is_empty() {
            continue;
        }

        comments.push(Comment {
            author: item
                .by
                .clone()
                .filter(|author| !author.is_empty())
                .unwrap_or_else(|| "unknown".to_string()),
            text: cleaned_text,
            published_at: item.time.unwrap_or_default(),
            depth: node.depth,
            ancestor_has_next_sibling: node.ancestor_has_next_sibling,
            is_last_sibling: node.is_last_sibling,
        });
    }

    Some(comments)
}

fn clean_comment_text(text: &str) -> String {
    let paragraph_normalized = text
        .replace("<p>", "\n")
        .replace("</p>", "")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");

    let without_tags = strip_html_tags(&paragraph_normalized);
    let decoded = decode_html_entities(&without_tags);

    let compacted = decoded
        .lines()
        .map(str::trim)
        .scan(false, |last_blank, line| {
            if line.is_empty() {
                if *last_blank {
                    return Some(None);
                }
                *last_blank = true;
                return Some(Some(""));
            }

            *last_blank = false;
            Some(Some(line))
        })
        .flatten()
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    compacted.trim().to_string()
}

fn strip_html_tags(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut in_tag = false;

    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }

    output
}

fn decode_html_entities(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}
