use chrono::Local;
use owo_colors::OwoColorize;
use std::fmt;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::registry::LookupSpan;

pub fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(
                "info,railway_server=debug,railway_hazel=debug,\
                 railway_protocol=debug,railway_game_logic=debug",
            )
        });

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_ansi(true)
        .event_format(ColorfulFormatter)
        .init();
}

pub struct ColorfulFormatter;

impl<S, N> FormatEvent<S, N> for ColorfulFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let ts = Local::now()
            .format("%H:%M:%S%.3f")
            .to_string()
            .bright_black()
            .to_string();

        let lvl = level_tag(*event.metadata().level());

        let target = event.metadata().target();
        let short = target
            .strip_prefix("railway_server::")
            .or_else(|| target.strip_prefix("railway_hazel::"))
            .or_else(|| target.strip_prefix("railway_protocol::"))
            .or_else(|| target.strip_prefix("railway_game_logic::"))
            .unwrap_or(target);
        let tgt = short.blue().to_string();

        let mut msg_buf = String::new();
        {
            struct Buf<'a>(&'a mut String);
            impl fmt::Write for Buf<'_> {
                fn write_str(&mut self, s: &str) -> fmt::Result {
                    self.0.push_str(s);
                    Ok(())
                }
            }
            let mut buf = Buf(&mut msg_buf);
            let tmp = Writer::new(&mut buf);
            ctx.field_format().format_fields(tmp, event)?;
        }

        let msg = paint(&msg_buf);

        write!(writer, "{} {} {} {}", ts, lvl, tgt, msg)?;
        writeln!(writer)
    }
}

fn level_tag(level: Level) -> String {
    match level {
        Level::ERROR => "[ERROR]".bright_red().bold().to_string(),
        Level::WARN  => "[WARN ]".bright_yellow().bold().to_string(),
        Level::INFO  => "[INFO ]".bright_blue().bold().to_string(),
        Level::DEBUG => "[DEBUG]".magenta().bold().to_string(),
        Level::TRACE => "[TRACE]".bright_black().bold().to_string(),
    }
}

fn paint(raw: &str) -> String {
    let text = raw.trim_end_matches('\n');

    if is_sep(text) {
        return text.magenta().bold().to_string();
    }

    if let Some((indent, label, colon, value)) = try_label_value(text) {
        return format!(
            "{}{}{}{}",
            indent,
            label.bright_white().bold(),
            colon.yellow().bold(),
            paint_tokens(value),
        );
    }

    paint_tokens(text)
}

fn is_sep(s: &str) -> bool {
    let t = s.trim();
    if t.len() < 6 {
        return false;
    }
    let first = t.chars().next().unwrap();
    if first.is_alphanumeric() {
        return false;
    }
    t.chars().all(|c| c == first)
}

fn try_label_value(s: &str) -> Option<(&str, &str, &str, &str)> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len && bytes[i] == b' ' {
        i += 1;
    }
    let indent = &s[..i];
    if i >= len {
        return None;
    }

    let label_start = i;
    while i < len && bytes[i] != b':' {
        i += 1;
    }
    if i >= len || i == label_start {
        return None;
    }
    let label_part = &s[label_start..i];
    if !label_part.chars().any(|c| c.is_alphabetic()) {
        return None;
    }
    if !label_part.ends_with(' ') {
        return None;
    }

    i += 1;
    let value = &s[i..];
    Some((indent, label_part, ":", value))
}

fn paint_tokens(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        if url_at(&chars, i) {
            let end = take_while(&chars, i, |c| !c.is_whitespace());
            let piece: String = chars[i..end].iter().collect();
            out.push_str(&piece.cyan().underline().to_string());
            i = end;
            continue;
        }

        if ch == '(' {
            if let Some(end) = find_close(&chars, i, '(', ')') {
                let piece: String = chars[i..end].iter().collect();
                out.push_str(&piece.bright_black().to_string());
                i = end;
                continue;
            }
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == '[' {
            if let Some(end) = find_close(&chars, i, '[', ']') {
                let piece: String = chars[i..end].iter().collect();
                out.push_str(&piece.bright_blue().to_string());
                i = end;
                continue;
            }
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == '`' {
            if let Some(end) = find_close(&chars, i, '`', '`') {
                let piece: String = chars[i..end].iter().collect();
                out.push_str(&piece.yellow().to_string());
                i = end;
                continue;
            }
            out.push(ch);
            i += 1;
            continue;
        }

        if ch.is_ascii_digit()
            || (ch == '-' && i + 1 < len && chars[i + 1].is_ascii_digit())
            || (ch == '0' && i + 1 < len && matches!(chars[i + 1], 'x' | 'X'))
        {
            let end = take_while(&chars, i, |c| {
                c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_' | '%' | ',')
            });
            let token: String = chars[i..end].iter().collect();

            if token.ends_with('%') {
                out.push_str(&token.bright_blue().bold().to_string());
            } else if token.contains('.') && token.chars().filter(|c| *c == '.').count() >= 3 {
                out.push_str(&token.cyan().bold().to_string());
            } else if token.contains(':') && token.chars().any(|c| c.is_ascii_digit()) {
                out.push_str(&token.cyan().bold().to_string());
            } else if token.starts_with("0x") || token.starts_with("0X") {
                out.push_str(&token.bright_magenta().to_string());
            } else {
                out.push_str(&token.bright_green().to_string());
            }
            i = end;
            continue;
        }

        if let Some(end) = word_at_chars(&chars, i, "true") {
            out.push_str(&"true".green().bold().to_string());
            i = end;
            continue;
        }
        if let Some(end) = word_at_chars(&chars, i, "false") {
            out.push_str(&"false".red().bold().to_string());
            i = end;
            continue;
        }
        if let Some(end) = word_at_chars(&chars, i, "enabled") {
            out.push_str(&"enabled".green().bold().to_string());
            i = end;
            continue;
        }
        if let Some(end) = word_at_chars(&chars, i, "disabled") {
            out.push_str(&"disabled".red().bold().to_string());
            i = end;
            continue;
        }

        if (ch == 'v' || ch == 'V') && i + 1 < len && chars[i + 1].is_ascii_digit() {
            let end =
                take_while(&chars, i, |c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
            let piece: String = chars[i..end].iter().collect();
            out.push_str(&piece.bright_green().bold().to_string());
            i = end;
            continue;
        }

        if let Some((_kw, end)) = bold_kw_at_chars(&chars, i) {
            let piece: String = chars[i..end].iter().collect();
            out.push_str(&piece.white().bold().to_string());
            i = end;
            continue;
        }

        out.push(ch);
        i += 1;
    }

    out
}

fn take_while(chars: &[char], start: usize, pred: fn(char) -> bool) -> usize {
    let mut i = start;
    while i < chars.len() && pred(chars[i]) {
        i += 1;
    }
    i
}

fn url_at(chars: &[char], pos: usize) -> bool {
    let s: String = chars[pos..].iter().collect();
    s.starts_with("http://") || s.starts_with("https://")
}

fn find_close(chars: &[char], open: usize, oc: char, cc: char) -> Option<usize> {
    if chars[open] != oc {
        return None;
    }
    let mut depth = 1;
    let mut i = open + 1;
    while i < chars.len() {
        if chars[i] == oc {
            depth += 1;
        } else if chars[i] == cc {
            depth -= 1;
        }
        if depth == 0 {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn word_at_chars(chars: &[char], pos: usize, w: &str) -> Option<usize> {
    let w_chars: Vec<char> = w.chars().collect();
    if pos + w_chars.len() > chars.len() {
        return None;
    }
    if chars[pos..pos + w_chars.len()] == w_chars[..] {
        let end = pos + w_chars.len();
        if end >= chars.len() || !chars[end].is_alphanumeric() && chars[end] != '_' {
            return Some(end);
        }
    }
    None
}

fn bold_kw_at_chars(chars: &[char], pos: usize) -> Option<(&'static str, usize)> {
    const WS: &[&str] = &[
        "OK", "ok", "SUCCESS", "success", "FAILED", "failed", "ERROR",
        "connected", "disconnected", "started", "stopped",
        "listening", "starting", "shutting down",
        "received", "registered", "loaded", "bound", "exited",
        "active", "initiated",
    ];
    for &w in WS {
        if let Some(end) = word_at_chars(chars, pos, w) {
            return Some((w, end));
        }
    }
    None
}
