use anyhow::Result;
use std::path::Path;

/// A single message from a Claude session transcript.
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

/// Read transcript info from the most recent session file:
/// last assistant message + aggregated usage stats.
pub fn read_transcript_info(project_dir: &Path, msg_count: usize) -> Result<TranscriptInfo> {
    let latest = match find_latest_jsonl(project_dir)? {
        Some(f) => f,
        None => return Ok(TranscriptInfo::default()),
    };

    let content = std::fs::read_to_string(&latest)?;
    let lines: Vec<&str> = content.lines().collect();

    let mut assistant_messages = Vec::new();
    let mut usage = TranscriptUsage::default();

    // Scan all lines for usage, collect assistant messages from the end
    for line in &lines {
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            // Accumulate usage from any message that has it
            accumulate_usage(&value, &mut usage);

            let role = value
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("");

            if role == "assistant" || role == "user" {
                usage.message_count += 1;
            }
        }
    }

    // Read last N assistant messages from the end
    for line in lines.iter().rev() {
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            let role = value
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string();

            if role == "assistant" {
                let content_text = extract_content_text(&value);
                if !content_text.is_empty() {
                    assistant_messages.push(TranscriptMessage {
                        role,
                        content: content_text,
                    });
                }

                if assistant_messages.len() >= msg_count {
                    break;
                }
            }
        }
    }

    assistant_messages.reverse();

    let last_message = assistant_messages
        .last()
        .map(|m| truncate_message(&m.content, 300))
        .unwrap_or_default();

    Ok(TranscriptInfo {
        last_message,
        usage,
    })
}

/// Read the last N assistant messages from a session transcript (legacy API).
pub fn read_last_messages(project_dir: &Path, count: usize) -> Result<Vec<TranscriptMessage>> {
    let latest = match find_latest_jsonl(project_dir)? {
        Some(f) => f,
        None => return Ok(Vec::new()),
    };

    let content = std::fs::read_to_string(&latest)?;
    let mut assistant_messages = Vec::new();

    for line in content.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            let role = value
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string();

            if role == "assistant" {
                let content_text = extract_content_text(&value);
                if !content_text.is_empty() {
                    assistant_messages.push(TranscriptMessage {
                        role,
                        content: content_text,
                    });
                }

                if assistant_messages.len() >= count {
                    break;
                }
            }
        }
    }

    assistant_messages.reverse();
    Ok(assistant_messages)
}

/// Find the most recent .jsonl file in a directory.
fn find_latest_jsonl(dir: &Path) -> Result<Option<std::path::PathBuf>> {
    let mut jsonl_files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();

    jsonl_files.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    Ok(jsonl_files.into_iter().next())
}

/// Extract text content from a message value.
fn extract_content_text(value: &serde_json::Value) -> String {
    if let Some(content) = value.get("content") {
        if let Some(s) = content.as_str() {
            return s.to_string();
        }

        if let Some(arr) = content.as_array() {
            let mut texts = Vec::new();
            for block in arr {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    texts.push(text.to_string());
                }
            }
            return texts.join("\n");
        }
    }

    String::new()
}

/// Accumulate token usage from a transcript line's usage field.
fn accumulate_usage(value: &serde_json::Value, usage: &mut TranscriptUsage) {
    if let Some(u) = value.get("usage") {
        if let Some(input) = u.get("input_tokens").and_then(|v| v.as_u64()) {
            usage.input_tokens += input;
        }
        if let Some(output) = u.get("output_tokens").and_then(|v| v.as_u64()) {
            usage.output_tokens += output;
        }
    }

    // Some transcript formats store cost at the top level or in metadata
    if let Some(cost) = value
        .get("costUSD")
        .or_else(|| value.get("cost_usd"))
        .and_then(|v| v.as_f64())
    {
        *usage.total_cost_usd.get_or_insert(0.0) += cost;
    }
}

/// Truncate a message to max_chars, appending "..." if truncated.
fn truncate_message(msg: &str, max_chars: usize) -> String {
    if msg.len() <= max_chars {
        msg.to_string()
    } else {
        let mut truncated = msg[..max_chars].to_string();
        truncated.push_str("...");
        truncated
    }
}
