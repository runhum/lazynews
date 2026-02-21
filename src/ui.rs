use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::style::Color;

pub const POST_SELECTED_COLOR: Color = Color::Rgb(255, 149, 0);
pub const POST_META_COLOR: Color = Color::Rgb(140, 140, 140);
pub const SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

pub fn format_age(unix_seconds: u64) -> String {
    if unix_seconds == 0 {
        return "-".into();
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());

    let elapsed = now.saturating_sub(unix_seconds);

    match elapsed {
        0..=59 => format!("{elapsed}s ago"),
        60..=3_599 => format!("{}m ago", elapsed / 60),
        3_600..=86_399 => format!("{}h ago", elapsed / 3_600),
        86_400..=604_799 => format!("{}d ago", elapsed / 86_400),
        604_800..=2_591_999 => format!("{}w ago", elapsed / 604_800),
        2_592_000..=31_535_999 => format!("{}mo ago", elapsed / 2_592_000),
        _ => format!("{}y ago", elapsed / 31_536_000),
    }
}
