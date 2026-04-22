use anyhow::Result;
use std::path::Path;

use crate::session::provider::SessionProvider;

/// A single message from a session transcript.
#[derive(Debug, Clone)]
pub struct TranscriptMessage {
    pub role: String,
    pub content: String,
}

/// Aggregated usage statistics from a session transcript.
#[derive(Debug, Clone, Default)]
pub struct TranscriptUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_cost_usd: Option<f64>,
    pub message_count: usize,
}

/// Combined transcript info for display.
#[derive(Debug, Clone, Default)]
pub struct TranscriptInfo {
    pub last_message: String,
    pub usage: TranscriptUsage,
}

#[derive(Debug, Clone)]
struct ParsedTranscriptEntry {
    role: String,
    content: String,
    usage: TranscriptUsage,
    counts_as_message: bool,
}

/// Read transcript info from the most recent session file:
/// last assistant message + aggregated usage stats.
pub fn read_transcript_info(
    provider: SessionProvider,
    project_dir: &Path,
    msg_count: usize,
) -> Result<TranscriptInfo> {
    let content = match read_latest_jsonl_content(project_dir)? {
        Some(content) => content,
        None => return Ok(TranscriptInfo::default()),
    };

    let mut assistant_messages = Vec::new();
    let mut usage = TranscriptUsage::default();
    let lines: Vec<&str> = content.lines().collect();

    for line in &lines {
        if let Some(entry) = parse_transcript_entry(provider, line) {
            usage.input_tokens += entry.usage.input_tokens;
            usage.output_tokens += entry.usage.output_tokens;
            if let Some(cost) = entry.usage.total_cost_usd {
                *usage.total_cost_usd.get_or_insert(0.0) += cost;
            }
            if entry.counts_as_message {
                usage.message_count += 1;
            }
        }
    }

    for line in lines.iter().rev() {
        let Some(entry) = parse_transcript_entry(provider, line) else {
            continue;
        };

        if entry.role == "assistant" && !entry.content.is_empty() {
            assistant_messages.push(TranscriptMessage {
                role: entry.role,
                content: entry.content,
            });
        }

        if assistant_messages.len() >= msg_count {
            break;
        }
    }

    assistant_messages.reverse();

    let last_message = assistant_messages
        .last()
        .map(|message| truncate_message(&message.content, 300))
        .unwrap_or_default();

    Ok(TranscriptInfo {
        last_message,
        usage,
    })
}

/// Read the last N assistant messages from a session transcript.
pub fn read_last_messages(
    provider: SessionProvider,
    project_dir: &Path,
    count: usize,
) -> Result<Vec<TranscriptMessage>> {
    let content = match read_latest_jsonl_content(project_dir)? {
        Some(content) => content,
        None => return Ok(Vec::new()),
    };

    let mut assistant_messages = Vec::new();

    for line in content.lines().rev() {
        let Some(entry) = parse_transcript_entry(provider, line) else {
            continue;
        };

        if entry.role == "assistant" && !entry.content.is_empty() {
            assistant_messages.push(TranscriptMessage {
                role: entry.role,
                content: entry.content,
            });
        }

        if assistant_messages.len() >= count {
            break;
        }
    }

    assistant_messages.reverse();
    Ok(assistant_messages)
}

fn read_latest_jsonl_content(project_dir: &Path) -> Result<Option<String>> {
    let latest = match find_latest_jsonl(project_dir)? {
        Some(file) => file,
        None => return Ok(None),
    };

    use std::io::{BufReader, Read, Seek, SeekFrom};

    let file = std::fs::File::open(&latest)?;
    let file_len = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    const MAX_READ_BYTES: u64 = 10 * 1024 * 1024;

    let content = if file_len > MAX_READ_BYTES {
        let mut reader = BufReader::new(&file);
        reader.seek(SeekFrom::End(-(MAX_READ_BYTES as i64)))?;
        let mut tail = String::new();
        reader.read_to_string(&mut tail)?;
        if let Some(pos) = tail.find('\n') {
            tail[pos + 1..].to_string()
        } else {
            tail
        }
    } else {
        std::fs::read_to_string(&latest)?
    };

    Ok(Some(content))
}

/// Find the most recent .jsonl file in a directory.
fn find_latest_jsonl(dir: &Path) -> Result<Option<std::path::PathBuf>> {
    let mut jsonl_files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();

    jsonl_files.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|metadata| metadata.modified()).ok();
        let b_time = b.metadata().and_then(|metadata| metadata.modified()).ok();
        b_time.cmp(&a_time)
    });

    Ok(jsonl_files.into_iter().next())
}

fn parse_transcript_entry(provider: SessionProvider, line: &str) -> Option<ParsedTranscriptEntry> {
    if line.trim().is_empty() {
        return None;
    }

    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    match provider {
        SessionProvider::Claude | SessionProvider::Codex => parse_claude_compatible_entry(&value),
        SessionProvider::Pi => parse_pi_entry(&value),
    }
}

fn parse_claude_compatible_entry(value: &serde_json::Value) -> Option<ParsedTranscriptEntry> {
    let role = value.get("role").and_then(|role| role.as_str())?.to_string();
    Some(ParsedTranscriptEntry {
        content: extract_content_text(value.get("content")?),
        counts_as_message: matches!(role.as_str(), "assistant" | "user"),
        usage: extract_usage(value),
        role,
    })
}

fn parse_pi_entry(value: &serde_json::Value) -> Option<ParsedTranscriptEntry> {
    if value.get("type").and_then(|entry_type| entry_type.as_str()) != Some("message") {
        return None;
    }

    let message = value.get("message")?;
    let role = message
        .get("role")
        .and_then(|role| role.as_str())?
        .to_string();

    Some(ParsedTranscriptEntry {
        content: message
            .get("content")
            .map(extract_content_text)
            .unwrap_or_default(),
        counts_as_message: matches!(role.as_str(), "assistant" | "user"),
        usage: extract_usage(message),
        role,
    })
}

/// Extract text content from a message value.
fn extract_content_text(content: &serde_json::Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_string();
    }

    if let Some(blocks) = content.as_array() {
        let mut texts = Vec::new();
        for block in blocks {
            let is_text_block = block
                .get("type")
                .and_then(|block_type| block_type.as_str())
                .map(|block_type| block_type == "text")
                .unwrap_or(true);
            if !is_text_block {
                continue;
            }
            if let Some(text) = block.get("text").and_then(|text| text.as_str()) {
                texts.push(text.to_string());
            }
        }
        return texts.join("\n");
    }

    String::new()
}

/// Extract token usage from a transcript entry or nested message object.
fn extract_usage(value: &serde_json::Value) -> TranscriptUsage {
    let mut usage = TranscriptUsage::default();

    if let Some(usage_value) = value.get("usage") {
        if let Some(input) = usage_value
            .get("input_tokens")
            .or_else(|| usage_value.get("input"))
            .and_then(|tokens| tokens.as_u64())
        {
            usage.input_tokens += input;
        }

        if let Some(output) = usage_value
            .get("output_tokens")
            .or_else(|| usage_value.get("output"))
            .and_then(|tokens| tokens.as_u64())
        {
            usage.output_tokens += output;
        }

        if let Some(cost) = usage_value
            .get("cost")
            .and_then(|cost| cost.get("total"))
            .and_then(|cost| cost.as_f64())
        {
            usage.total_cost_usd = Some(cost);
        }
    }

    if let Some(cost) = value
        .get("costUSD")
        .or_else(|| value.get("cost_usd"))
        .and_then(|cost| cost.as_f64())
    {
        *usage.total_cost_usd.get_or_insert(0.0) += cost;
    }

    usage
}

/// Truncate a message to max_chars (character count, not bytes), appending "..." if truncated.
fn truncate_message(msg: &str, max_chars: usize) -> String {
    let char_count = msg.chars().count();
    if char_count <= max_chars {
        msg.to_string()
    } else {
        let mut truncated: String = msg.chars().take(max_chars).collect();
        truncated.push_str("...");
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn write_transcript(dir: &tempfile::TempDir, name: &str, lines: &[&str]) {
        std::fs::write(dir.path().join(name), format!("{}\n", lines.join("\n"))).unwrap();
    }

    #[test]
    fn parses_claude_transcript_usage_and_preview() {
        let dir = tempfile::tempdir().unwrap();
        write_transcript(
            &dir,
            "sess.jsonl",
            &[
                r#"{"role":"user","content":"Review this","usage":{"input_tokens":11,"output_tokens":0}}"#,
                r#"{"role":"assistant","content":[{"text":"Done."}],"usage":{"input_tokens":5,"output_tokens":7},"costUSD":0.12}"#,
            ],
        );

        let info = read_transcript_info(SessionProvider::Claude, dir.path(), 1).unwrap();
        assert_eq!(info.last_message, "Done.");
        assert_eq!(info.usage.input_tokens, 16);
        assert_eq!(info.usage.output_tokens, 7);
        assert_eq!(info.usage.message_count, 2);
        assert_eq!(info.usage.total_cost_usd, Some(0.12));
    }

    #[test]
    fn parses_pi_message_preview_and_usage() {
        let dir = tempfile::tempdir().unwrap();
        write_transcript(
            &dir,
            "2026-04-22_pi.jsonl",
            &[
                r#"{"type":"session","version":3,"cwd":"/tmp/project"}"#,
                r#"{"type":"message","id":"1","parentId":null,"timestamp":"2026-04-22T10:00:00Z","message":{"role":"user","content":"Fix the tests"}}"#,
                r#"{"type":"message","id":"2","parentId":"1","timestamp":"2026-04-22T10:00:01Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"Patched the failing assertion."},{"type":"toolCall","name":"bash","arguments":{"cmd":"cargo test"}}],"usage":{"input":123,"output":45,"cost":{"total":0.34}}}}"#,
            ],
        );

        let info = read_transcript_info(SessionProvider::Pi, dir.path(), 1).unwrap();
        assert_eq!(info.last_message, "Patched the failing assertion.");
        assert_eq!(info.usage.input_tokens, 123);
        assert_eq!(info.usage.output_tokens, 45);
        assert_eq!(info.usage.total_cost_usd, Some(0.34));
        assert_eq!(info.usage.message_count, 2);
    }

    #[test]
    fn pi_mixed_entries_do_not_fail_and_missing_usage_stays_zero() {
        let dir = tempfile::tempdir().unwrap();
        write_transcript(
            &dir,
            "2026-04-22_pi.jsonl",
            &[
                r#"{"type":"session","version":3,"cwd":"/tmp/project"}"#,
                r#"{"type":"model_change","id":"1","parentId":null,"provider":"openai","modelId":"gpt-5"}"#,
                r#"{"type":"message","id":"2","parentId":"1","timestamp":"2026-04-22T10:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"Looks good."}]}}"#,
                r#"{"type":"message","id":"3","parentId":"2","timestamp":"2026-04-22T10:00:02Z","message":{"role":"custom","content":"ignored"}}"#,
            ],
        );

        let info = read_transcript_info(SessionProvider::Pi, dir.path(), 1).unwrap();
        assert_eq!(info.last_message, "Looks good.");
        assert_eq!(info.usage.input_tokens, 0);
        assert_eq!(info.usage.output_tokens, 0);
        assert_eq!(info.usage.total_cost_usd, None);
        assert_eq!(info.usage.message_count, 1);
    }

    #[test]
    fn read_last_messages_respects_newest_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        write_transcript(&dir, "older.jsonl", &[r#"{"role":"assistant","content":"old"}"#]);
        std::thread::sleep(Duration::from_millis(15));
        write_transcript(&dir, "newer.jsonl", &[r#"{"role":"assistant","content":"new"}"#]);

        let messages = read_last_messages(SessionProvider::Claude, dir.path(), 1).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "new");
    }
}
