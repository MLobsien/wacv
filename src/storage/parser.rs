use crate::storage::chat::*;
use regex::Regex;
use std::sync::LazyLock;

const LRM: char = '\u{200e}';

static MEDIA_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<Anhang:\s*(.+?)>|<Attachment:\s*(.+?)>").expect("invalid media regex")
});
static DURATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d+)\s*(Min|min|Sek|sek)").expect("invalid duration regex")
});
static EDITED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\u{200e}<Diese Nachricht wurde bearbeitet\.>\s*$|\u{200e}<This message was edited\.>\s*$",
    )
    .expect("invalid edited regex")
});
static OPTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\u{200E}OPTION:\s*(.*?)\s*\((\d+)\s*\S+\)\s*$").expect("invalid option regex")
});

/// Parse the _chat.txt content from a WhatsApp export.
///
/// Message boundaries are "\r\n"; multi-line message text keeps bare "\n"
/// inside the message, so splitting on "\r\n" yields exactly one chunk per
/// message with no leftover line endings.
pub fn parse_chat(content: &str) -> Vec<Message> {
    // (timestamp, sender, text, attachment_hint)
    let mut raw_messages: Vec<(i64, String, String, bool)> = Vec::new();

    for line in content.split("\r\n") {
        if line.is_empty() {
            continue;
        }

        // An LRM directly before the timestamp marks an attachment message.
        let attachment_hint = line.starts_with(LRM);
        let line = line.trim_start_matches(LRM);

        // Parse the leading "[DD.MM.YY, HH:MM:SS]" from the fixed offsets.
        let Some((timestamp, header_len)) = parse_timestamp(line) else {
            continue;
        };
        let body = &line[header_len..];

        if let Some(sep) = body.find(':') {
            let sender = body[..sep].trim().to_string();
            let text = body[sep + 1..].trim_start_matches(' ').to_string();
            raw_messages.push((timestamp, sender, text, attachment_hint));
        }
    }

    // Classify each message. Messages with empty text or a media-omitted
    // placeholder (classify_message returns None) are skipped - WhatsApp emits
    // bare header lines ("[ts] Sender:") before media, which must not render.
    raw_messages
        .into_iter()
        .filter_map(|(timestamp, sender, text, attachment_hint)| {
            let kind = classify_message(&text, attachment_hint)?;
            Some(Message {
                timestamp,
                sender: match kind {
                    // System notices carry no user; keep the sender on the
                    // encryption notice: WhatsApp attributes it to the other
                    // participant, which `Chat::my_name` uses to identify the
                    // contact in a 1:1 chat.
                    MessageKind::System(_) => None,
                    _ => Some(normalize_sender(&sender).to_string()),
                },
                kind,
            })
        })
        .collect()
}

/// Parse the leading "[DD.MM.YY, HH:MM:SS]" from fixed byte offsets.
/// Returns the wall-clock time as a Unix timestamp (UTC interpretation) and
/// the header length. No regex, no chrono: WhatsApp already writes the
/// local time, so the parsed integers are used as-is.
fn parse_timestamp(line: &str) -> Option<(i64, usize)> {
    // "[DD.MM.YY, HH:MM:SS]" is exactly 20 bytes.
    const HEADER: usize = 20;
    let b = line.as_bytes();
    if b.len() < HEADER || b[0] != b'[' || b[19] != b']'
        || b[3] != b'.' || b[6] != b'.' || b[9] != b',' || b[10] != b' '
        || b[13] != b':' || b[16] != b':'
    {
        return None;
    }
    let num = |i: usize| -> Option<u32> {
        if b[i].is_ascii_digit() && b[i + 1].is_ascii_digit() {
            Some(((b[i] - b'0') as u32) * 10 + (b[i + 1] - b'0') as u32)
        } else {
            None
        }
    };
    let day = num(1)?;
    let month = num(4)?;
    let yy = num(7)?;
    let hour = num(11)?;
    let minute = num(14)?;
    let second = num(17)?;
    let year = 2000 + yy as i32; // WhatsApp exports 2-digit years
    let days = days_from_civil(year, month as i32, day as i32);
    Some((
        days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64,
        HEADER,
    ))
}

/// Days since 1970-01-01 (Howard Hinnant's civil calendar algorithm).
fn days_from_civil(y: i32, m: i32, d: i32) -> i64 {
    let y = (y - if m <= 2 { 1 } else { 0 }) as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = ((m + 9) % 12) as u32; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as u32 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as i64; // [0, 146096]
    era * 146097 + doe - 719468
}

fn classify_message(text: &str, attachment_hint: bool) -> Option<MessageKind> {
    let text_clean = text.trim_start_matches(LRM).trim();

    // Empty message
    if text_clean.is_empty() {
        return None;
    }

    // Encryption notice (always starts with LRM in the text)
    if is_encryption_notice(text_clean) {
        return Some(MessageKind::EncryptionNotice);
    }

    // Media attachment: <Anhang: ...> or <Attachment: ...>
    if let Some((filename, caption)) = extract_media(text) {
        return Some(MessageKind::Media { filename, caption });
    }

    // An LRM directly before the timestamp marks an attachment message. If
    // no media tag was found, the media was omitted from the export
    // ("Bild weggelassen") - drop it just like an empty message.
    if attachment_hint {
        return None;
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
        if is_deletion_message(&system_text) {
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

fn is_deletion_message(text: &str) -> bool {
    text == "Diese Nachricht wurde gelöscht."
        || text == "Du hast diese Nachricht gelöscht."
        || text == "This message was deleted."
        || text == "You deleted this message."
}

/// Extract media attachment filename plus optional caption.
/// Returns (filename, caption) - caption is None for media without text.
/// WhatsApp appends the media tag at the END of the message, so any
/// text (including multi-line) appears BEFORE it: "Text <Anhang: file>".
fn extract_media(text: &str) -> Option<(String, Option<String>)> {
    // Match <Anhang: filename> or <Attachment: filename>
    let caps = MEDIA_RE.captures(text)?;
    let filename = caps
        .get(1)
        .or_else(|| caps.get(2))?
        .as_str()
        .trim()
        .to_string();
    if filename.is_empty() {
        return None;
    }
    // Caption is the text BEFORE the media tag (may span lines)
    let full_match = caps.get(0)?;
    let before = &text[..full_match.start()];
    let caption = before.trim().trim_end_matches(LRM).trim().to_string();
    let caption = if caption.is_empty() {
        None
    } else {
        Some(strip_edited_suffix(&caption).unwrap_or(caption))
    };
    Some((filename, caption))
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

        let kind = if is_missed || no_answer {
            CallKind::Missed
        } else {
            CallKind::Incoming
        };

        // Try to extract duration
        let duration_secs = DURATION_RE.captures(text).and_then(|caps| {
            let num: u64 = caps[1].parse().ok()?;
            let unit = caps[2].to_lowercase();
            match unit.as_str() {
                "min" => Some(num * 60),
                "sek" => Some(num),
                _ => None,
            }
        });
        return Some(CallInfo {
            kind,
            duration_secs,
        });
    }

    None
}

fn strip_edited_suffix(text: &str) -> Option<String> {
    // Edited messages end with LRM + "<Diese Nachricht wurde bearbeitet.>"
    // or LRM + "<This message was edited.>"
    if EDITED_RE.is_match(text) {
        let base = EDITED_RE.replace(text, "").trim().to_string();
        return Some(base);
    }


    None
}

/// Detect if the message is a voting/poll (contains LRM + OPTION: lines).
fn detect_voting(text: &str) -> bool {
    text.lines()
        .any(|line| line.contains('\u{200e}') && line.trim().contains("OPTION:"))
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
    for line in text.lines() {
        if line.trim().contains('\u{200e}') && line.trim().contains("OPTION:") {
            in_options = true;
            if let Some(caps) = OPTION_RE.captures(line.trim()) {
                options.push(VoteOption {
                    text: caps[1].to_string(),
                    votes: caps[2].parse().unwrap_or(0),
                });
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
/// or "Download/WhatsApp Chat - Bob" (Android picker fallback path).
pub fn chat_name_from_filename(filename: &str) -> String {
    // Use only the basename: Android's content-URI fallback can return a
    // path like "Download/WhatsApp Chat - Bob".
    let basename = filename.rsplit(['/', '\\']).next().unwrap_or(filename);

    // WhatsApp exports are always "WhatsApp Chat - <name>.zip".
    let stripped_prefix = basename.strip_prefix("WhatsApp Chat - ").unwrap_or(basename);
    let name = stripped_prefix
        .strip_suffix(".zip")
        .unwrap_or(stripped_prefix)
        .trim_start_matches(LRM)
        .to_string();
    strip_download_suffix(&name)
}

fn strip_download_suffix(name: &str) -> String {
    if let Some(open) = name.rfind('(') {
        let tail = &name[open..];
        let digits = tail.trim_start_matches('(').trim_end_matches(')');
        let is_dup = tail.starts_with('(')
            && tail.ends_with(')')
            && !digits.is_empty()
            && digits.chars().all(|c| c.is_ascii_digit());
        if is_dup {
            return name[..open].trim_end().to_string();
        }
    }
    name.to_string()
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
        // Wall-clock time parsed as-is (no timezone shift): 2024-10-08 23:08:02
        assert_eq!(messages[0].timestamp, 1_728_428_882);
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
        let input =
            "[24.09.25, 17:50:12] Bob: \u{200e}<Anhang: 00000005-PHOTO-2025-09-24-17-50-12.jpg>";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].kind, MessageKind::Media { .. }));
        if let MessageKind::Media { filename, caption } = &messages[0].kind {
            assert_eq!(filename, "00000005-PHOTO-2025-09-24-17-50-12.jpg");
            assert!(caption.is_none());
        }
    }

    #[test]
    fn test_parse_media_with_caption() {
        // Real WhatsApp format: caption text BEFORE the media tag
        let input =
            "[24.09.25, 17:51:00] Bob: Caption text here \u{200e}<Anhang: 00000005-PHOTO-2025-09-24-17-50-12.jpg>";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        if let MessageKind::Media { filename, caption } = &messages[0].kind {
            assert_eq!(filename, "00000005-PHOTO-2025-09-24-17-50-12.jpg");
            assert_eq!(caption.as_deref(), Some("Caption text here"));
        } else {
            panic!("expected media message");
        }
    }

    #[test]
    fn test_parse_media_with_multiline_caption() {
        // Multi-line caption: continuation lines precede the media tag
        let input = "[24.09.25, 17:52:00] Bob: First line\nSecond line \u{200e}<Anhang: VID-20250924-WA0001.mp4>";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        if let MessageKind::Media { caption, .. } = &messages[0].kind {
            assert_eq!(caption.as_deref(), Some("First line\nSecond line"));
        } else {
            panic!("expected media message");
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
        let input =
            "[30.04.25, 16:46:59] Charlie: \u{200e}Missed voice call. \u{200e}Tap to call back.";
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
        assert_eq!(
            chat_name_from_filename("Download/WhatsApp Chat - Bob"),
            "Bob"
        );
        assert_eq!(
            chat_name_from_filename("WhatsApp Chat - David"),
            "David"
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
\u{200e}OPTION: Option A (5 Stimmen)
\u{200e}OPTION: Option B (3 Stimmen)
\u{200e}OPTION: Option C (1 Stimme)";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].kind, MessageKind::Voting { .. }));
        assert_eq!(messages[0].sender.as_deref(), Some("Alice"));
        if let MessageKind::Voting { question, options } = &messages[0].kind {
            assert_eq!(question, "What is your favorite color?");
            assert_eq!(options.len(), 3);
            assert_eq!(options[0].text, "Option A");
            assert_eq!(options[0].votes, 5);
            assert_eq!(options[1].text, "Option B");
            assert_eq!(options[1].votes, 3);
            assert_eq!(options[2].text, "Option C");
            assert_eq!(options[2].votes, 1);
        }
    }

    #[test]
    fn test_parse_voting_english() {
        let input = "[15.06.25, 11:00:00] Bob: POLL: Best day?
\u{200e}OPTION: Option A (10 votes)
\u{200e}OPTION: Option B (2 votes)";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        if let MessageKind::Voting { question, options } = &messages[0].kind {
            assert_eq!(question, "Best day?");
            assert_eq!(options.len(), 2);
            assert_eq!(options[0].text, "Option A");
            assert_eq!(options[0].votes, 10);
            assert_eq!(options[1].text, "Option B");
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
                    VoteOption {
                        text: "A".to_string(),
                        votes: 10,
                    },
                    VoteOption {
                        text: "B".to_string(),
                        votes: 5,
                    },
                ],
            },
        };
        assert_eq!(msg.preview_text(), "Best option?");
    }

    #[test]
    fn test_parse_media_empty_header_line() {
        // WhatsApp exports media-only messages as a header with no text:
        //   [ts] Sender:
        // followed by the real media line. The empty header must be ignored.
        let input = "[02.01.26, 21:02:02] Robin:\r\n[02.01.26, 21:02:02] Robin: \u{200e}<Anhang: 00000055-PHOTO-2026-01-02-21-02-02.jpg>";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].sender.as_deref(), Some("Robin"));
        assert!(matches!(messages[0].kind, MessageKind::Media { .. }));
    }

    #[test]
    fn test_media_header_not_merged_into_previous() {
        // A text message followed by a bare media header line (no text) must
        // stay separate, and the bare header must not leak into the text.
        let input = "[02.01.26, 22:01:00] Morgan: Burner\r\n[02.01.26, 21:02:02] Robin:\r\n[02.01.26, 21:02:02] Robin: \u{200e}<Anhang: 00000055-PHOTO-2026-01-02-21-02-02.jpg>";
        let messages = parse_chat(input);
        assert_eq!(messages.len(), 2);
        if let MessageKind::Text { content, .. } = &messages[0].kind {
            assert_eq!(content, "Burner");
        } else {
            panic!("expected text message");
        }
        assert_eq!(messages[0].sender.as_deref(), Some("Morgan"));
        assert_eq!(messages[1].sender.as_deref(), Some("Robin"));
        assert!(matches!(messages[1].kind, MessageKind::Media { .. }));
    }

}

#[test]
fn test_parse_voting_question_on_separate_line() {
    // ABSTIMMUNG: on its own line, question on the next line
    // question on next line
    let input = "[14.06.25, 23:29:09] ~ Eli: \u{200e}ABSTIMMUNG:\nQuestion text?\n\u{200e}OPTION: Option A (1 Stimme)\n\u{200e}OPTION: Option B (7 Stimmen)";
    let messages = parse_chat(input);
    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0].kind, MessageKind::Voting { .. }));
    assert_eq!(messages[0].sender.as_deref(), Some("Eli"));
    if let MessageKind::Voting { question, options } = &messages[0].kind {
        assert_eq!(question, "Question text?");
        assert_eq!(options[1].votes, 7);
    }
}

#[test]
fn test_parse_media_caption_real_format() {
    // Multi-line captions with the media tag at the end
    let samples = [
        ("[06.04.25, 22:52:28] Alex: Caption one. \u{200e}<Anhang: 00000009-PHOTO-2025-04-06-22-52-28.jpg>", Some("Caption one.")),
        ("[31.07.25, 16:46:58] Alex: Caption two. \u{200e}<Anhang: 00000031-PHOTO-2025-07-31-16-46-58.jpg>", Some("Caption two.")),
        ("[18.11.24, 15:48:19] Ben: \u{200e}<Anhang: 00000005-PHOTO-2024-11-18-15-48-19.jpg>", None),
        // Multi-line: caption spans two lines, tag at end
        ("[24.04.25, 07:48:54] ~ Sara: First caption line.\nSecond caption line. \u{200e}<Anhang: 00000109-PHOTO-2025-04-24-07-48-54.jpg>", Some("First caption line.\nSecond caption line.")),
    ];
    for (input, expected) in samples {
        let msgs = parse_chat(input);
        assert_eq!(msgs.len(), 1, "input: {input}");
        let kind = &msgs[0].kind;
        match kind {
            MessageKind::Media { caption, .. } => {
                assert_eq!(caption.as_deref(), expected, "input: {input}");
            }
            other => panic!("expected Media, got {other:?} for: {input}"),
        }
    }
}

#[test]
fn test_timestamp_is_wall_clock_as_utc() {
    // WhatsApp writes local wall-clock times. The parser stores them as
    // though they were UTC so the epoch equals the wall clock: the value
    // corresponds to the written time interpreted in UTC (no shift).
    let samples = [
        ("[08.10.24, 23:08:02] Alice: hi", 1_728_428_882),
        ("[27.08.22, 08:32:14] Bob: hi", 1_661_589_134),
        ("[24.04.25, 07:48:54] Carol: hi", 1_745_480_934),
    ];
    for (input, expected) in samples {
        let msgs = parse_chat(input);
        assert_eq!(msgs.len(), 1, "input: {input}");
        assert_eq!(msgs[0].timestamp, expected, "input: {input}");
    }
}

#[test]
fn test_parse_media_omitted_placeholder_skipped() {
    // LRM + "Bild weggelassen" (media omitted from the export) carries no
    // browsable content - it must be skipped like an empty message.
    let input = "\u{200e}[03.12.24, 14:50:03] ~\u{202f}Sender: \u{200e}Bild weggelassen";
    let messages = parse_chat(input);
    assert!(messages.is_empty());
}