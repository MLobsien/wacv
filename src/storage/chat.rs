use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chat {
    pub name: String,
    pub messages: Vec<Message>,
}

/// Strip WhatsApp export decorations from a sender name: LRM (U+200E),
/// direction marks (U+202A/U+202C), and the "~" / "~\u{202f}" prefix
/// WhatsApp adds to some contacts.
pub fn normalize_sender(s: &str) -> &str {
    let s = s.trim_start_matches('\u{200e}');
    let s = s.trim_start_matches('\u{202a}');
    let s = s.trim_start_matches('\u{202e}');
    let s = s.trim_end_matches('\u{202c}');
    let s = s.trim_end_matches('\u{200e}');
    // WhatsApp prefixes some contacts with "~" (optionally followed by a
    // narrow no-break space or plain space).
    if let Some(rest) = s.strip_prefix('~') {
        let rest = rest.trim_start_matches(|c| c == '\u{202f}' || c == ' ');
        if !rest.is_empty() {
            return rest;
        }
    }
    s
}

impl Chat {
    pub fn new(name: String, messages: Vec<Message>) -> Self {
        Self { name, messages }
    }

    pub fn display_name(&self) -> &str {
        // Strip any leading \u200e from chat name
        self.name.trim_start_matches('\u{200e}')
    }

    pub fn last_message_preview(&self) -> Option<&str> {
        self.messages.last().map(|m| m.preview_text())
    }

    pub fn last_timestamp(&self) -> Option<i64> {
        self.messages.last().map(|m| m.timestamp)
    }

    /// Determine the user's name in a 2-person chat.
    /// The chat is named after the other person, so the user is whoever is not the chat name.
    pub fn my_name(&self) -> Option<String> {
        // Signal 1: the sender of "you deleted this message" notices is always the user.
        for m in &self.messages {
            if let MessageKind::Deleted { by_sender: true } = &m.kind {
                if let Some(s) = m.sender.as_deref() {
                    return Some(normalize_sender(s).to_string());
                }
            }
        }

        let senders: std::collections::BTreeSet<&str> = self.messages.iter()
            .filter_map(|m| m.sender.as_deref())
            .map(normalize_sender)
            .collect();

        match senders.len() {
            0 => None,
            1 => {
                let sender = senders.iter().next()?;
                if *sender == self.display_name() {
                    None
                } else {
                    Some(sender.to_string())
                }
            }
            2 => {
                let chat_name = self.display_name();
                // Signal 2: chat named after the other person.
                if let Some(other) = senders.iter().find(|s| **s == chat_name) {
                    return senders.iter().find(|s| **s != *other).map(|s| s.to_string());
                }
                // Signal 3: phone-number-named chat — the very first message
                // (encryption notice) is always sent by the contact.
                let contact = self.messages.first()
                    .and_then(|m| m.sender.as_deref())
                    .map(normalize_sender);
                if let Some(contact) = contact {
                    if let Some(me) = senders.iter().find(|s| **s != contact) {
                        return Some(me.to_string());
                    }
                }
                // Fallback: first sender that isn't the chat name.
                senders.iter().find(|s| **s != chat_name).map(|s| s.to_string())
            }
            _ => None, // group chat (3+ senders)
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub timestamp: i64,
    pub sender: Option<String>,
    pub kind: MessageKind,
}

impl Message {
    pub fn preview_text(&self) -> &str {
        match &self.kind {
            MessageKind::Text { content, .. } => content,
            MessageKind::System(text) => text,
            MessageKind::Call(_) => match &self.kind {
                MessageKind::Call(c) => c.preview_text(),
                _ => unreachable!(),
            },
            MessageKind::Media { filename, caption } => {
                if let Some(caption) = caption {
                    if !caption.is_empty() {
                        return caption.as_str();
                    }
                }
                if filename.contains("STICKER") {
                    "Sticker"
                } else if filename.contains("PHOTO") || filename.contains("IMAGE") {
                    "Image"
                } else if filename.contains("VIDEO") {
                    "Video"
                } else if filename.contains("AUDIO") {
                    "Voice message"
                } else {
                    "Attachment"
                }
            }
            MessageKind::Deleted { .. } => "This message was deleted",
            MessageKind::EncryptionNotice => "",
            MessageKind::Voting { question, .. } => {
                if question.is_empty() { "Poll" } else { question.as_str() }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MessageKind {
    /// Regular text message. edited=true if message was edited.
    Text { content: String, edited: bool },
    /// System message (group created, added, icon changed, contact notice, etc.)
    System(String),
    /// Call info (voice/video call, missed, duration)
    Call(CallInfo),
    /// Media attachment (image, video, audio, sticker), optionally with a caption
    Media { filename: String, caption: Option<String> },
    /// Message deleted notice
    Deleted { by_sender: bool },
    /// Encryption notice (first message in every chat)
    EncryptionNotice,
    /// Voting/poll message with question and options
    Voting { question: String, options: Vec<VoteOption> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoteOption {
    pub text: String,
    pub votes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallInfo {
    pub kind: CallKind,
    pub duration_secs: Option<u64>,
}

impl CallInfo {
    pub fn preview_text(&self) -> &str {
        match self.kind {
            CallKind::Missed => "Missed call",
            CallKind::Outgoing => "Outgoing call",
            CallKind::Incoming => "Incoming call",
        }
    }
    }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CallKind {
    Missed,
    Outgoing,
    Incoming,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_name_chat_named_after_other() {
        let chat = Chat::new(
            "Emma".into(),
            vec![
                Message { timestamp: 1, sender: Some("Emma".into()), kind: MessageKind::Text { content: "hi".into(), edited: false } },
                Message { timestamp: 2, sender: Some("Alex".into()), kind: MessageKind::Text { content: "hey".into(), edited: false } },
            ],
        );
        assert_eq!(chat.my_name(), Some("Alex".to_string()));
    }

    #[test]
    fn test_my_name_phone_chat_uses_first_message_sender() {
        // Phone-number-named chat: the chat name matches neither sender, so
        // we fall back to the first message (encryption notice) sender.
        let chat = Chat::new(
            "+49 1234".into(),
            vec![
                Message { timestamp: 1, sender: Some("~\u{202f}Emma".into()), kind: MessageKind::EncryptionNotice },
                Message { timestamp: 2, sender: Some("Emma".into()), kind: MessageKind::Text { content: "hi".into(), edited: false } },
                Message { timestamp: 3, sender: Some("Alex".into()), kind: MessageKind::Text { content: "hey".into(), edited: false } },
            ],
        );
        assert_eq!(chat.my_name(), Some("Alex".to_string()));
    }

    #[test]
    fn test_my_name_self_deleted_signal() {
        // The sender of a "you deleted this message" notice wins even when
        // the contact's name sorts before the user's.
        let chat = Chat::new(
            "+49 1234".into(),
            vec![
                Message { timestamp: 1, sender: Some("Emma".into()), kind: MessageKind::Text { content: "hi".into(), edited: false } },
                Message { timestamp: 2, sender: Some("Alex".into()), kind: MessageKind::Text { content: "hey".into(), edited: false } },
                Message { timestamp: 3, sender: Some("~\u{202f}Alex".into()), kind: MessageKind::Deleted { by_sender: true } },
            ],
        );
        assert_eq!(chat.my_name(), Some("Alex".to_string()));
    }

    #[test]
    fn test_normalize_sender() {
        assert_eq!(normalize_sender("~\u{202f}Mia"), "Mia");
        assert_eq!(normalize_sender("~Fred Baker"), "Fred Baker");
        assert_eq!(normalize_sender("\u{200e}Du"), "Du");
        assert_eq!(normalize_sender("\u{202a}+49 176 1234567\u{202c}"), "+49 176 1234567");
        assert_eq!(normalize_sender("Alex"), "Alex");
    }
}
