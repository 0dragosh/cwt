use anyhow::Result;
use std::path::Path;

/// A single message from a Claude session transcript.
#[derive(Debug, Clone)]
pub struct TranscriptMessage {
    pub role: String,
    pub content: String,
}

/// Read the last N assistant messages from a session transcript.
/// Claude Code stores sessions as .jsonl files.
pub fn read_last_messages(project_dir: &Path, count: usize) -> Result<Vec<TranscriptMessage>> {
    // Find the most recent .jsonl file
    let mut jsonl_files: Vec<_> = std::fs::read_dir(project_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();

    // Sort by modification time, most recent first
    jsonl_files.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    let Some(latest) = jsonl_files.first() else {
        return Ok(Vec::new());
    };

    let content = std::fs::read_to_string(latest)?;
    let mut assistant_messages = Vec::new();

    for line in content.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }

        // Try to parse as JSON
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

/// Extract text content from a message value.
fn extract_content_text(value: &serde_json::Value) -> String {
    // Content can be a string or an array of content blocks
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
