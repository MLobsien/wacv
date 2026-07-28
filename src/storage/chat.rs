use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chat {
    pub name: String,
    pub messages: Vec<Message>,
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
        let senders: std::collections::BTreeSet<&str> = self.messages.iter()
            .filter_map(|m| m.sender.as_deref())
            .collect();

        if senders.len() == 2 {
            let chat_name = self.display_name();
            senders.iter()
                .find(|s| **s != chat_name)
                .map(|s| s.to_string())
        } else if senders.len() == 1 {
            let sender = senders.iter().next()?;
            if *sender == self.display_name() {
                None
            } else {
                Some(sender.to_string())
            }
        } else {
            None
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
            MessageKind::Media { filename, .. } => {
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
    /// Media attachment (image, video, audio, sticker)
    Media { filename: String },
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
