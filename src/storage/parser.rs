use crate::storage::chat::*;
use chrono::NaiveDateTime;
use regex::Regex;

const LRM: char = '\u{200e}';

/// Parse the _chat.txt content from a WhatsApp export
pub fn parse_chat(content: &str) -> Vec<Message> {
    let line_re = Regex::new(
        r"^\[(\d{2}\.\d{2}\.\d{2}, \d{2}:\d{2}:\d{2})\] (.+?): (.*)$",
    )
    .expect("invalid line regex");

    let date_re = Regex::new(
        r"^\[(\d{2}\.\d{2}\.\d{2}, \d{2}:\d{2}:\d{2})\]",
    )
    .expect("invalid date regex");

    // First pass: collect raw lines, handling multi-line messages
    let mut raw_messages: Vec<(String, String, String)> = Vec::new();

    for line in content.lines() {
        // Strip leading \u200e that sometimes precedes the timestamp
        let trimmed = line.trim_start_matches(LRM);

        if let Some(caps) = line_re.captures(trimmed) {
            let ts = caps[1].to_string();
            let sender = caps[2].to_string();
            let text = caps[3].to_string();
            raw_messages.push((ts, sender, text));
        } else if let Some(caps) = date_re.captures(trimmed) {
            // Timestamp found but no sender:message pattern — likely empty message
            let ts = caps[1].to_string();
            let rest = &trimmed[caps[0].len()..];
            if let Some(sep_pos) = rest.find(": ") {
                let sender = rest[..sep_pos].to_string();
                let text = rest[sep_pos + 2..].to_string();
                raw_messages.push((ts, sender, text));
            } else {
                // Continuation of previous message if no sender found
                if let Some(last) = raw_messages.last_mut() {
                    last.2.push('\n');
                    last.2.push_str(line);
                }
            }
        } else {
            // Continuation of multi-line message
            if let Some(last) = raw_messages.last_mut() {
                last.2.push('\n');
                last.2.push_str(line);
            }
        }
    }

    // Second pass: classify each raw message
    raw_messages
        .into_iter()
        .filter_map(|(ts_str, sender, text)| {
            let timestamp = parse_timestamp(&ts_str)?;
            let kind = classify_message(&sender, &text);
            Some(Message {
                timestamp,
                sender: kind.as_ref().and_then(|k| match k {
                    MessageKind::System(_) | MessageKind::EncryptionNotice => None,
                    _ => Some(normalize_sender(&sender).to_string()),
                }),
                kind: kind.unwrap_or(MessageKind::Text {
                    content: text,
                    edited: false,
                }),
            })
        })
        .collect()
}

fn parse_timestamp(ts: &str) -> Option<i64> {
    // Format: "DD.MM.YY, HH:MM:SS"
    // chrono expects "YY-MM-DD HH:MM:SS"
    let normalized = ts
        .replace('.', "-")
        .replace(',', "")
        .trim()
        .to_string();

    // Parse with year-first format for NaiveDateTime
    // Input is "DD-MM-YY HH:MM:SS"
    let parts: Vec<&str> = normalized.splitn(3, '-').collect();
    if parts.len() < 3 {
        return None;
    }
    let day = parts[0];
    let month = parts[1];
    let rest = parts[2]; // "YY HH:MM:SS"

    let rest_parts: Vec<&str> = rest.splitn(2, ' ').collect();
    if rest_parts.len() < 2 {
        return None;
    }
    let year = rest_parts[0];
    let time = rest_parts[1];

    // Convert 2-digit year to 4-digit
    let year_full = if let Ok(y) = year.parse::<i32>() {
        if y < 100 {
            2000 + y
        } else {
            y
        }
    } else {
        return None;
    };

    let _formatted = format!("{:04}-{:02}-{} {} {}", year_full, month.parse::<i32>().unwrap_or(0), day, time, "00");
    // Actually simpler: just parse DD.MM.YY HH:MM:SS directly with custom parsing
    let naive = NaiveDateTime::parse_from_str(
        &format!("{:02}.{:02}.{:04} {}", 
            day.parse::<u32>().unwrap_or(1),
            month.parse::<u32>().unwrap_or(1),
            year_full,
            time
        ),
        "%d.%m.%Y %H:%M:%S",
    )
    .ok()?;

    Some(naive.and_utc().timestamp())
}

fn classify_message(sender: &str, text: &str) -> Option<MessageKind> {
    let text_stripped = text.trim_start_matches(LRM).trim();
    let text_clean = text_stripped.trim();

    // Empty message
    if text_clean.is_empty() {
        return None;
    }

    // Encryption notice (always starts with LRM in the text)
    if text.contains(LRM) && is_encryption_notice(text_clean) {
        return Some(MessageKind::EncryptionNotice);
    }

    // Media attachment: <Anhang: ...> or <Attachment: ...>
    if let Some(filename) = extract_media_filename(text) {
        return Some(MessageKind::Media { filename });
    }

    // Check for call messages (language-specific)
    if let Some(call) = detect_call(text_clean) {
        return Some(MessageKind::Call(call));
    }

    // Voting/poll message: lines starting with LRM + OPTION:
    if detect_voting(text) {
        let (question, options) = parse_voting(text);
        return Some(MessageKind::Voting { question, options });
    }

    // Check for system messages: text starts with LRM
    if text.starts_with(LRM) {
        let system_text = text.trim_start_matches(LRM).trim().to_string();

        // Check for deletion messages
        if is_deletion_message(&system_text, sender) {
            let by_sender = system_text.contains("Du hast") || system_text.contains("You deleted");
            return Some(MessageKind::Deleted { by_sender });
        }

        // Group system message
        return Some(MessageKind::System(system_text));
    }

    // Edited message: ends with LRM + edited text
    if let Some(base_content) = strip_edited_suffix(text) {
        return Some(MessageKind::Text {
            content: base_content,
            edited: true,
        });
    }

    // Regular text message
    // Also strip any remaining LRM prefix
    let content = text.trim_start_matches(LRM).to_string();
    Some(MessageKind::Text {
        content,
        edited: false,
    })
}

fn is_encryption_notice(text: &str) -> bool {
    let text = text.trim().trim_start_matches(LRM);
    text.starts_with("Nachrichten und Anrufe sind Ende-zu-Ende-verschlüsselt")
        || text.starts_with("Messages and calls are end-to-end encrypted")
}

fn is_deletion_message(text: &str, _sender: &str) -> bool {
    text == "Diese Nachricht wurde gelöscht."
        || text == "Du hast diese Nachricht gelöscht."
        || text == "This message was deleted."
        || text == "You deleted this message."
}

fn extract_media_filename(text: &str) -> Option<String> {
    // Match <Anhang: filename> or <Attachment: filename>
    let re = Regex::new(r"<Anhang:\s*(.+?)>|<Attachment:\s*(.+?)>").expect("invalid media regex");
    if let Some(caps) = re.captures(text) {
        let filename = caps.get(1).or_else(|| caps.get(2))?.as_str().trim().to_string();
        if !filename.is_empty() {
            return Some(filename);
        }
    }
    None
}

fn detect_call(text: &str) -> Option<CallInfo> {
    let text = text.trim();

    // German call patterns
    // "Sprachanruf. 2 Min." or "Sprachanruf. 2 min." or "Sprachanruf. Keine Antwort"
    // "Verpasster Sprachanruf. Zum Zurückrufen tippen"
    // "Videoanruf. 5 Min."
    // English: "Voice call. 2 min." "Missed voice call. Tap to call back"
    if text.contains("Sprachanruf")
        || text.contains("Voice call")
        || text.contains("voice call")
        || text.contains("Videoanruf")
        || text.contains("Video call")
        || text.contains("video call")
    {
        let is_missed = text.contains("Verpasster") || text.contains("Missed");
        let no_answer = text.contains("Keine Antwort") || text.contains("No answer");

        let kind = if is_missed {
            CallKind::Missed
        } else if no_answer {
            CallKind::Missed
        } else {
            CallKind::Incoming
        };

        // Try to extract duration
        let duration_re = Regex::new(r"(\d+)\s*(Min|min|Sek|sek)").expect("invalid duration regex");
        let duration_secs = duration_re.captures(text).and_then(|caps| {
            let num: u64 = caps[1].parse().ok()?;
            let unit = caps[2].to_lowercase();
            match unit.as_str() {
                "min" => Some(num * 60),
                "sek" => Some(num),
                _ => None,
            }
        });

        return Some(CallInfo { kind, duration_secs });
    }

    None
}

fn strip_edited_suffix(text: &str) -> Option<String> {
    // Edited messages end with LRM + "<Diese Nachricht wurde bearbeitet.>"
    // or LRM + "<This message was edited.>"
    let edited_re = Regex::new(
        r"\u{200e}<Diese Nachricht wurde bearbeitet\.>\s*$|\u{200e}<This message was edited\.>\s*$",
    )
    .expect("invalid edited regex");

    if edited_re.is_match(text) {
        let base = edited_re.replace(text, "").trim().to_string();
        return Some(base);
    }

    // Also try without LRM
    let edited_re2 = Regex::new(
        r"<Diese Nachricht wurde bearbeitet\.>\s*$|<This message was edited\.>\s*$",
    )
    .expect("invalid edited regex2");

    if edited_re2.is_match(text) {
        let base = edited_re2.replace(text, "").trim().to_string();
        return Some(base);
    }

    None
}

/// Detect if the message is a voting/poll (contains LRM + OPTION: lines).
fn detect_voting(text: &str) -> bool {
    text.lines().any(|line| line.contains('\u{200e}') && line.trim().contains("OPTION:"))
}

/// Parse a voting message into question and options.
///
/// Input format (language-specific):
///   ABSTIMMUNG: Question?
///   LRM + OPTION: Option text (5 Stimmen)
///   LRM + OPTION: Option text (1 Stimme)
///
/// English variant:
///   POLL: Question?
///   LRM + OPTION: Option text (5 votes)
///   LRM + OPTION: Option text (1 vote)
fn parse_voting(text: &str) -> (String, Vec<VoteOption>) {
    let mut question = String::new();
    let mut options = Vec::new();
    let mut in_options = false;

    // Match OPTION lines: LRM + OPTION: text (N words)
    // Where "words" is any language variant (votes, Stimmen, vote, Stimme, etc.)
    let option_re = Regex::new(
        r"^\u{200e}OPTION:\s*(.*?)\s*\((\d+)\s*\S+\)\s*$"
    ).expect("invalid option regex");

    for line in text.lines() {
        if line.trim().contains('\u{200e}') && line.trim().contains("OPTION:") {
            in_options = true;
            if let Some(caps) = option_re.captures(line.trim()) {
                options.push(VoteOption {
                    text: caps[1].to_string(),
                    votes: caps[2].parse().unwrap_or(0),
                });
            } else {
                // Fallback: everything after "OPTION: " prefix
                if let Some(idx) = line.trim().find("OPTION:") {
                    let after = line.trim()[idx + "OPTION:".len()..].trim().to_string();
                    options.push(VoteOption { text: after, votes: 0 });
                }
            }
        } else if !in_options {
            let cleaned = line.trim_start_matches(LRM).trim();
            if !cleaned.is_empty() {
                // Strip language-specific poll prefix
                let cleaned = cleaned
                    .strip_prefix("ABSTIMMUNG:")
                    .or_else(|| cleaned.strip_prefix("POLL:"))
                    .unwrap_or(cleaned)
                    .trim()
                    .to_string();
                if !cleaned.is_empty() {
                    if !question.is_empty() {
                        question.push('\n');
                    }
                    question.push_str(&cleaned);
                }
            }
        }
    }

    (question, options)
}

/// Parse the chat name from a zip filename like "WhatsApp Chat - Alice.zip"
pub fn chat_name_from_filename(filename: &str) -> String {
    let name = filename
        .strip_prefix("WhatsApp Chat - ")
        .unwrap_or(filename)
        .strip_suffix(".zip")
        .unwrap_or(filename)
        .to_string();
    // Strip leading \u200e
    name.trim_start_matches(LRM).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_message() {
        let input = "[08.10.24, 23:08:02] Alice: Hello world!";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        if let MessageKind::Text { content, edited } = &messages[0].kind {
            assert_eq!(content, "Hello world!");
            assert!(!edited);
        } else {
            panic!("expected text message");
        }
        assert_eq!(messages[0].sender.as_deref(), Some("Alice"));
    }

    #[test]
    fn test_parse_system_message() {
        let input = "[23.11.24, 18:59:08] Group: \u{200e}Group created the group.";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].kind, MessageKind::System(_)));
        assert!(messages[0].sender.is_none());
    }

    #[test]
    fn test_parse_media() {
        let input = "[24.09.25, 17:50:12] Bob: \u{200e}<Anhang: 00000005-PHOTO-2025-09-24-17-50-12.jpg>";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].kind, MessageKind::Media { .. }));
        if let MessageKind::Media { filename } = &messages[0].kind {
            assert_eq!(filename, "00000005-PHOTO-2025-09-24-17-50-12.jpg");
        }
    }

    #[test]
    fn test_parse_edited_message() {
        let input = "[12.08.25, 23:15:29] Alice: All good \u{200e}<This message was edited.>";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        if let MessageKind::Text { content, edited } = &messages[0].kind {
            assert_eq!(content, "All good");
            assert!(*edited);
        } else {
            panic!("expected edited text message");
        }
    }

    #[test]
    fn test_parse_call_message() {
        let input = "[30.04.25, 16:47:11] Bob: \u{200e}Voice call. \u{200e}2 min.";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        if let MessageKind::Call(call) = &messages[0].kind {
            assert_eq!(call.kind, CallKind::Incoming);
            assert_eq!(call.duration_secs, Some(120));
        } else {
            panic!("expected call message");
        }
    }

    #[test]
    fn test_parse_missed_call() {
        let input = "[30.04.25, 16:46:59] Charlie: \u{200e}Missed voice call. \u{200e}Tap to call back.";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        if let MessageKind::Call(call) = &messages[0].kind {
            assert_eq!(call.kind, CallKind::Missed);
        } else {
            panic!("expected missed call message");
        }
    }

    #[test]
    fn test_parse_deleted_message() {
        let input = "[30.01.26, 07:41:49] Charlie: \u{200e}This message was deleted.";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].kind, MessageKind::Deleted { .. }));
    }

    #[test]
    fn test_parse_self_deleted() {
        let input = "[13.06.25, 22:24:57] Bob: \u{200e}You deleted this message.";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        if let MessageKind::Deleted { by_sender } = &messages[0].kind {
            assert!(*by_sender);
        } else {
            panic!("expected deleted message");
        }
    }

    #[test]
    fn test_chat_name_from_filename() {
        assert_eq!(
            chat_name_from_filename("WhatsApp Chat - Alice.zip"),
            "Alice"
        );
        assert_eq!(
            chat_name_from_filename("WhatsApp Chat - Best Friends.zip"),
            "Best Friends"
        );
    }

    #[test]
    fn test_multi_line_message() {
        let input = "[08.10.25, 18:15:09] Bob: Limits we haven't covered yet.
        Recursive formulas:
        You start at step n.";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        if let MessageKind::Text { content, .. } = &messages[0].kind {
            assert!(content.contains("Recursive formulas"));
            assert!(content.contains("step n"));
        }
    }

    #[test]
    fn test_parse_voting_german() {
        let input = "[15.06.25, 10:30:00] Alice: ABSTIMMUNG: What is your favorite color?
\u{200e}OPTION: Red (5 Stimmen)
\u{200e}OPTION: Blue (3 Stimmen)
\u{200e}OPTION: Green (1 Stimme)";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].kind, MessageKind::Voting { .. }));
        assert_eq!(messages[0].sender.as_deref(), Some("Alice"));
        if let MessageKind::Voting { question, options } = &messages[0].kind {
            assert_eq!(question, "What is your favorite color?");
            assert_eq!(options.len(), 3);
            assert_eq!(options[0].text, "Red");
            assert_eq!(options[0].votes, 5);
            assert_eq!(options[1].text, "Blue");
            assert_eq!(options[1].votes, 3);
            assert_eq!(options[2].text, "Green");
            assert_eq!(options[2].votes, 1);
        }
    }

    #[test]
    fn test_parse_voting_english() {
        let input = "[15.06.25, 11:00:00] Bob: POLL: Best day?
\u{200e}OPTION: Monday (10 votes)
\u{200e}OPTION: Tuesday (2 votes)";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        if let MessageKind::Voting { question, options } = &messages[0].kind {
            assert_eq!(question, "Best day?");
            assert_eq!(options.len(), 2);
            assert_eq!(options[0].text, "Monday");
            assert_eq!(options[0].votes, 10);
            assert_eq!(options[1].text, "Tuesday");
            assert_eq!(options[1].votes, 2);
        }
    }

    #[test]
    fn test_parse_voting_preview() {
        let msg = Message {
            timestamp: 0,
            sender: Some("Alice".to_string()),
            kind: MessageKind::Voting {
                question: "Best option?".to_string(),
                options: vec![
                    VoteOption { text: "A".to_string(), votes: 10 },
                    VoteOption { text: "B".to_string(), votes: 5 },
                ],
            },
        };
        assert_eq!(msg.preview_text(), "Best option?");
    }
}

    #[test]
    fn test_parse_voting_question_on_separate_line() {
        // Real format from 11t chat: ABSTIMMUNG: on its own line,
        // question on next line
        let input = "[14.06.25, 23:29:09] ~ Elias: ‎ABSTIMMUNG:\nElias?\n\u{200e}OPTION: Ja (1 Stimme)\n\u{200e}OPTION: Nein (7 Stimmen)";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].kind, MessageKind::Voting { .. }));
        assert_eq!(messages[0].sender.as_deref(), Some("Elias"));
        if let MessageKind::Voting { question, options } = &messages[0].kind {
            assert_eq!(question, "Elias?");
            assert_eq!(options.len(), 2);
            assert_eq!(options[0].text, "Ja");
            assert_eq!(options[0].votes, 1);
            assert_eq!(options[1].text, "Nein");
            assert_eq!(options[1].votes, 7);
        }
    }
