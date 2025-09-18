use colored::*;
use std::time::{SystemTime, UNIX_EPOCH};

const BANNER_WIDTH: usize = 47;

pub fn print_about() {
    let name = env!("CARGO_PKG_NAME").to_uppercase();
    let version = env!("CARGO_PKG_VERSION");
    let authors = env!("CARGO_PKG_AUTHORS");
    let homepage = option_env!("CARGO_PKG_HOMEPAGE").unwrap_or("");
    let repo = option_env!("CARGO_PKG_REPOSITORY").unwrap_or("");
    let license = "GPL-3.0-or-later";

    println!("{}", "=".repeat(BANNER_WIDTH).bright_black());
    println!(
        "{}",
        centered(&format!("{name} v{version}"), BANNER_WIDTH)
            .bold()
            .bright_green()
    );
    println!("{}", "=".repeat(BANNER_WIDTH).bright_black());
    // Split authors into "Name <email>" parts
    if let Some((org, email)) = split_author(authors) {
        println!(
            "{} {} - {} {}",
            "Copyright (C)".bright_yellow(),
            current_year(),
            org,
            email.blue()
        );
    } else {
        println!(
            "{} {} - {}",
            "Copyright (C)".bright_yellow(),
            current_year(),
            authors
        );
    }
    println!("{}", "-".repeat(BANNER_WIDTH).bright_black());
    let key = |k: &str| format!("{k:>11}:").bold().bright_yellow();
    println!("{} {}", key("License"), license.bright_black());
    if !homepage.is_empty() {
        println!(
            "{} {}",
            key("Homepage"),
            strip_https(homepage).bright_black()
        );
    }
    if !repo.is_empty() {
        println!("{} {}", key("Repository"), strip_https(repo).bright_black());
    }

    println!("{}", "-".repeat(BANNER_WIDTH).bright_black());
    println!(
        "{}",
        "This program comes with ABSOLUTELY NO WARRANTY."
            .red()
            .bold()
    );
    println!("This is free software, and you are welcome to");
    println!("redistribute it under certain conditions.");
    println!("See the LICENSE for details.");
    println!("{}", "-".repeat(BANNER_WIDTH).bright_black());
    println!(
        "{} {} {}",
        "Tip: run".bright_green(),
        "`eenv help`".cyan().bold(),
        "for all commands,".bright_green()
    );
    println!(
        "{} {} {}",
        "or".bright_green(),
        "`eenv init`".cyan().bold(),
        "to set up.".bright_green()
    );
    println!("{}", "=".repeat(BANNER_WIDTH).bright_black());
}

fn current_year() -> i32 {
    const AVG_YEAR_SECS: i64 = 31_556_952;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    1970 + (now / AVG_YEAR_SECS) as i32
}

fn centered(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        return text.to_string();
    }
    let total = width - len;
    let left = total / 2;
    let right = total - left;
    format!(
        "{:left$}{}{:right$}",
        "",
        text,
        "",
        left = left,
        right = right
    )
}

/// Strip https:// or http:// from URLs
fn strip_https(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
}

/// Split "Org <email>" into (Org, email)
fn split_author(s: &str) -> Option<(&str, &str)> {
    let start = s.find('<')?;
    let end = s.find('>')?;
    let org = s[..start].trim();
    let email = &s[start..=end];
    Some((org, email))
}
