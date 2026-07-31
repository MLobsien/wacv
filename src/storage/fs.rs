use crate::storage::chat::Chat;
use crate::storage::parser::{chat_name_from_filename, parse_chat};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::{fs, io::Read};

pub struct ChatStorage {
    data_dir: PathBuf,
    media_dir: PathBuf,
}

impl ChatStorage {
    pub fn new() -> Result<Self> {
        let data_dir = Self::get_data_dir()?;
        let media_dir = Self::get_media_dir()?;
        fs::create_dir_all(&data_dir).context("failed to create data dir")?;
        eprintln!("[WACV] ChatStorage::new() data={data_dir:?} media={media_dir:?}");
        fs::create_dir_all(&media_dir).context("failed to create media dir")?;
        Ok(Self {
            data_dir,
            media_dir,
        })
    }

    /// Import a chat from a zip file (full zip bytes)
    pub fn import_chat(&self, zip_bytes: &[u8], filename: &str) -> Result<String> {
        let chat_name = chat_name_from_filename(filename);
        let cursor = std::io::Cursor::new(zip_bytes);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| anyhow::anyhow!("invalid zip: {}", e))?;

        // Extract media files
        let chat_media = self.media_dir.join(&chat_name);
        fs::create_dir_all(&chat_media).context("failed to create chat media dir")?;

        let mut chat_text = String::new();

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).context("failed to read zip entry")?;
            let entry_name = file.name().to_string();

            if entry_name == "_chat.txt" {
                file.read_to_string(&mut chat_text)
                    .context("failed to read _chat.txt")?;
            } else if !entry_name.starts_with('.') && !entry_name.ends_with('/') {
                // Extract media file, stripping any directory prefix
                let basename = std::path::Path::new(&entry_name)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&entry_name);
                let media_path = chat_media.join(basename);
                let mut out = fs::File::create(&media_path)
                    .context(format!("failed to create media file: {}", entry_name))?;
                std::io::copy(&mut file, &mut out)
                    .context(format!("failed to extract media: {}", entry_name))?;
            }
        }

        if chat_text.is_empty() {
            anyhow::bail!("no _chat.txt found in zip");
        }

        let messages = parse_chat(&chat_text);
        let chat = Chat::new(chat_name.clone(), messages);

        // Save chat metadata
        self.save_chat(&chat)?;

        Ok(chat_name)
    }

    pub fn save_chat(&self, chat: &Chat) -> Result<()> {
        let chat_dir = self.data_dir.join(&chat.name);
        fs::create_dir_all(&chat_dir).context("failed to create chat dir")?;

        let bytes = serde_cbor::to_vec(chat).context("failed to serialize chat")?;
        fs::write(chat_dir.join("chat.cbor"), &bytes)
            .context("failed to write chat.cbor")?;

        Ok(())
    }

    pub fn list_chats(&self) -> Result<Vec<String>> {
        let mut chats = Vec::new();
        if !self.data_dir.exists() {
            return Ok(chats);
        }
        for entry in fs::read_dir(&self.data_dir).context("failed to read data dir")? {
            let entry = entry.context("failed to read entry")?;
            if entry.file_type()?.is_dir() {
                let chat_cbor = entry.path().join("chat.cbor");
                if chat_cbor.exists() {
                    if let Some(name) = entry.file_name().to_str() {
                        chats.push(name.to_string());
                    }
                }
            }
        }
        chats.sort();
        Ok(chats)
    }

    pub fn load_chat(&self, name: &str) -> Result<Chat> {
        let chat_path = self.data_dir.join(name).join("chat.cbor");
        let bytes = fs::read(&chat_path).context(format!("failed to read chat: {}", name))?;
        let chat: Chat =
            serde_cbor::from_slice(&bytes).context(format!("failed to deserialize chat: {}", name))?;
        Ok(chat)
    }

    pub fn delete_chat(&self, name: &str) -> Result<()> {
        let chat_dir = self.data_dir.join(name);
        if chat_dir.exists() {
            fs::remove_dir_all(&chat_dir).context(format!("failed to delete chat dir: {}", name))?;
        }
        // Media lives alongside chats, so remove it too.
        let media_dir = self.media_dir.join(name);
        if media_dir.exists() {
            fs::remove_dir_all(&media_dir).context(format!("failed to delete media dir: {}", name))?;
        }
        Ok(())
    }

    /// Get the path to a media file for a chat
    pub fn media_path(&self, chat_name: &str, filename: &str) -> PathBuf {
        self.media_dir.join(chat_name).join(filename)
    }

    /// Check if media file exists
    pub fn media_exists(&self, chat_name: &str, filename: &str) -> bool {
        self.media_path(chat_name, filename).exists()
    }

    fn get_data_dir() -> Result<PathBuf> {
        #[cfg(target_os = "android")]
        if let Some(dir) = crate::android::android_data_dir() {
            eprintln!("[WACV] get_data_dir: android path={:?}", dir.join("wacv").join("chats"));
            return Ok(dir.join("wacv").join("chats"));
        }
        let base = dirs::data_dir().context("failed to get data dir")?;
        eprintln!("[WACV] get_data_dir: desktop path={:?}", base.join("wacv").join("chats"));
        Ok(base.join("wacv").join("chats"))
    }

    fn get_media_dir() -> Result<PathBuf> {
        #[cfg(target_os = "android")]
        if let Some(dir) = crate::android::android_data_dir() {
            eprintln!("[WACV] get_media_dir: android path={:?}", dir.join("wacv").join("media"));
            return Ok(dir.join("wacv").join("media"));
        }
        let base = dirs::data_dir().context("failed to get data dir")?;
        eprintln!("[WACV] get_media_dir: desktop path={:?}", base.join("wacv").join("media"));
        Ok(base.join("wacv").join("media"))
    }
}
