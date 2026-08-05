use crate::storage::chat::Chat;
use crate::storage::parser::{chat_name_from_filename, parse_chat};
use anyhow::{Context, Result};
use std::io::Read;
use std::path::PathBuf;
use std::{fs, io};

/// Result of scanning a zip via its local file headers.
struct ZipScan {
    chat_text: String,
    /// (name inside the zip, uncompressed bytes)
    media: Vec<(String, Vec<u8>)>,
}

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

    /// Import a chat from a zip file (full zip bytes).
    ///
    /// Tries the strict central-directory parser first; if the archive is
    /// malformed (some WhatsApp exports have incomplete central directories),
    /// falls back to scanning the local file headers directly.
    pub fn import_chat(&self, zip_bytes: &[u8], filename: &str) -> Result<String> {
        let chat_name = chat_name_from_filename(filename);
        let chat_media = self.media_dir.join(&chat_name);
        fs::create_dir_all(&chat_media).context("failed to create chat media dir")?;

        let scan = match scan_zip_via_central_directory(zip_bytes) {
            Ok(scan) => scan,
            Err(central_err) => {
                eprintln!("[WACV] central-directory parse failed ({central_err}); falling back to local-header scan");
                scan_zip_via_local_headers(zip_bytes)?
            }
        };

        if scan.chat_text.is_empty() {
            anyhow::bail!("no _chat.txt found in zip");
        }

        for (entry_name, bytes) in &scan.media {
            let basename = std::path::Path::new(entry_name)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(entry_name);
            let media_path = chat_media.join(basename);
            fs::write(&media_path, bytes)
                .context(format!("failed to write media file: {}", entry_name))?;
        }

        let messages = parse_chat(&scan.chat_text);
        let chat = Chat::new(chat_name.clone(), messages);
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

    /// List chats together with the timestamp of their last message, so the
    /// UI can sort them by recency. The order is unspecified.
    pub fn list_chats_with_last_timestamp(&self) -> Result<Vec<(String, i64)>> {
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
                        let last_ts = self
                            .load_chat(name)
                            .ok()
                            .and_then(|c| c.last_timestamp())
                            .unwrap_or(0);
                        chats.push((name.to_string(), last_ts));
                    }
                }
            }
        }
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

/// Scan a zip using its central directory (strict).
fn scan_zip_via_central_directory(zip_bytes: &[u8]) -> Result<ZipScan> {
    let cursor = io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| anyhow::anyhow!("invalid zip: {}", e))?;

    let mut scan = ZipScan {
        chat_text: String::new(),
        media: Vec::new(),
    };
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("failed to read zip entry")?;
        // WhatsApp writes UTF-8 entry names without the ZIP UTF-8 flag (bit 11).
        // The `zip` crate then falls back to CP437 and mangles non-ASCII names,
        // so decode the raw bytes as UTF-8 instead of trusting `name()`.
        let entry_name = String::from_utf8_lossy(file.name_raw()).into_owned();
        if entry_name == "_chat.txt" {
            file.read_to_string(&mut scan.chat_text)
                .context("failed to read _chat.txt")?;
        } else if !entry_name.starts_with('.') && !entry_name.ends_with('/') {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).context("failed to read zip entry")?;
            scan.media.push((entry_name, bytes));
        }
    }
    Ok(scan)
}

/// Scan a zip by walking its local file headers. Tolerates malformed central
/// directories (incomplete WhatsApp exports): sizes are ignored and each entry
/// is inflated as a self-terminating deflate stream.
fn scan_zip_via_local_headers(zip_bytes: &[u8]) -> Result<ZipScan> {
    let mut scan = ZipScan {
        chat_text: String::new(),
        media: Vec::new(),
    };
    let mut pos = 0usize;

    while pos + 30 <= zip_bytes.len() {
        // Local file header signature: PK\x03\x04
        if zip_bytes[pos..pos + 4] != [0x50, 0x4b, 0x03, 0x04] {
            // Misaligned: a truncated deflate stream or a data descriptor may
            // have left us mid-entry. Re-synchronize on the next local header
            // instead of giving up, so `_chat.txt` is still recovered from
            // archives whose central directory is incomplete or corrupt.
            match find_next_local_header(zip_bytes, pos + 1) {
                Some(next) => {
                    pos = next;
                    continue;
                }
                None => break,
            }
        }

        let flags = u16::from_le_bytes([zip_bytes[pos + 6], zip_bytes[pos + 7]]);
        let method = u16::from_le_bytes([zip_bytes[pos + 8], zip_bytes[pos + 9]]);
        let name_len = u16::from_le_bytes([zip_bytes[pos + 26], zip_bytes[pos + 27]]) as usize;
        let extra_len = u16::from_le_bytes([zip_bytes[pos + 28], zip_bytes[pos + 29]]) as usize;

        let name_start = pos + 30;
        let data_start = name_start + name_len + extra_len;
        if data_start > zip_bytes.len() {
            // Bogus header (false `PK\x03\x04` inside media data): resync.
            pos += 4;
            continue;
        }
        let name = String::from_utf8_lossy(&zip_bytes[name_start..name_start + name_len]).into_owned();
        if !plausible_zip_name(&name) {
            pos += 4;
            continue;
        }

        // The local header's compressed-size field is unreliable for streaming
        // entries (data-descriptor mode writes 0). Instead find where the deflate
        // stream ends by inflating it.
        let (decoded, consumed) = match inflate_deflate(&zip_bytes[data_start..], method) {
            Ok(v) => v,
            Err(_) => {
                // Corrupt or unsupported entry: skip it and keep scanning.
                pos = data_start + 1;
                continue;
            }
        };
        pos = data_start + consumed;

        // Streaming entries (flag bit 3) append a data descriptor after the
        // compressed data: signature + crc + sizes. Skip it so the next loop
        // iteration lands exactly on the next local header.
        if flags & 0x0008 != 0 && pos + 4 <= zip_bytes.len() && zip_bytes[pos..pos + 4] == [0x50, 0x4b, 0x07, 0x08] {
            pos += 16; // 4-byte signature + crc(4) + compressed size(4) + uncompressed size(4)
        }

        if name == "_chat.txt" {
            scan.chat_text = String::from_utf8_lossy(&decoded).into_owned();
        } else if !name.starts_with('.') && !decoded.is_empty() {
            scan.media.push((name, decoded));
        }
    }
    Ok(scan)
}

/// Find the next `PK\x03\x04` signature at or after `from`. `memchr` scans for
/// the first byte (`'P'`) at SIMD speed, so re-synchronizing across multi-gigabyte
/// gaps in a corrupt archive stays fast.
fn find_next_local_header(data: &[u8], from: usize) -> Option<usize> {
    for idx in memchr::memchr_iter(0x50, &data[from..]) {
        let abs = from + idx;
        if abs + 4 <= data.len() && data[abs + 1] == 0x4b && data[abs + 2] == 0x03 && data[abs + 3] == 0x04 {
            return Some(abs);
        }
    }
    None
}

/// WhatsApp file names are printable (digits, `-`, `.`, base64 chars) but may
/// contain non-ASCII UTF-8 (umlauts, emoji). Reject obviously bogus names a
/// false `PK\x03\x04` signature might produce: empty, oversized, directory
/// entries, or strings with control characters.
fn plausible_zip_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.ends_with('/')
        && name.chars().all(|c| !c.is_control())
}

/// Inflate a deflate stream starting at `data`, returning the decoded bytes and
/// the number of bytes consumed. `read_to_end` stops at the stream's natural
/// end, so entries written with data descriptors are handled correctly.
fn inflate_deflate(data: &[u8], method: u16) -> Result<(Vec<u8>, usize)> {
    // Method 0 = stored (no compression).
    if method == 0 {
        return Ok((data.to_vec(), data.len()));
    }
    if method != 8 {
        anyhow::bail!("unsupported compression method: {method}");
    }

    let mut decoder = flate2::read::DeflateDecoder::new(data);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| anyhow::anyhow!("deflate error: {e}"))?;
    let consumed = decoder.total_in() as usize;
    Ok((out, consumed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip() -> Vec<u8> {
        let cursor = io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("_chat.txt", opts).unwrap();
        w.write_all(b"[25.07.25, 10:32:55] Alex: Hi\r\n[25.07.25, 10:33:00] Emma: Bye\r\n").unwrap();
        w.start_file("00000001-pic.jpg", opts).unwrap();
        w.write_all(b"fakejpegdata").unwrap();
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn central_directory_scan_handles_valid_zip() {
        let zip = make_zip();
        let scan = scan_zip_via_central_directory(&zip).expect("valid zip should parse");
        assert!(scan.chat_text.contains("Alex: Hi"));
        assert_eq!(scan.media.len(), 1);
        assert_eq!(scan.media[0].0, "00000001-pic.jpg");
    }

    #[test]
    fn local_headers_scan_recovers_from_truncated_cd() {
        let zip = make_zip();
        // Find the central directory start and keep only the bytes before it
        // (local file headers + data). This simulates a WhatsApp export whose
        // central directory is missing or inconsistent.
        let cd_marker = [0x50, 0x4b, 0x01, 0x02];
        let cd_pos = zip.windows(4).position(|w| w == cd_marker).expect("CD marker");
        let truncated = &zip[..cd_pos];

        let scan = scan_zip_via_local_headers(truncated).expect("scan should recover");
        assert!(scan.chat_text.contains("Alex: Hi"));
        assert_eq!(scan.media.len(), 1);
        assert_eq!(scan.media[0].1, b"fakejpegdata");
    }

    /// Build a WhatsApp-style zip: a UTF-8 entry name stored WITHOUT the ZIP
    /// UTF-8 flag (bit 11). The `zip` crate then decodes such names as CP437
    /// (e.g. `U\u{0308}` bytes `55 CC 88` become `U╠ê`), which is exactly the
    /// mangling that used to break media file lookups for non-ASCII names.
    fn make_whatsapp_style_zip() -> Vec<u8> {
        let cursor = io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("_chat.txt", opts).unwrap();
        w.write_all(b"[25.07.25, 10:32:55] Alex: Hi\r\n").unwrap();
        w.start_file("00000127-Allergen-U\u{0308}bersicht.pdf", opts).unwrap();
        w.write_all(b"fakepdfdata").unwrap();
        let mut bytes = w.finish().unwrap().into_inner();
        // Clear the UTF-8 flag (bit 11, 0x0800) in every local file header
        // (flags at +6) and central directory entry (flags at +8).
        let mut i = 0;
        while i + 4 <= bytes.len() {
            let sig = &bytes[i..i + 4];
            let flag_offset = if sig == b"PK\x03\x04" {
                Some(6)
            } else if sig == b"PK\x01\x02" {
                Some(8)
            } else {
                None
            };
            if let Some(off) = flag_offset {
                let pos = i + off;
                let flags = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
                bytes[pos..pos + 2].copy_from_slice(&(flags & !0x0800).to_le_bytes());
            }
            i += 1;
        }
        bytes
    }

    #[test]
    fn central_directory_scan_keeps_utf8_names_without_flag() {
        let zip = make_whatsapp_style_zip();
        // The correct, UTF-8-decoded media name must survive the scan.
        let scan = scan_zip_via_central_directory(&zip).expect("valid zip should parse");
        assert!(scan.chat_text.contains("Alex: Hi"));
        assert_eq!(scan.media.len(), 1);
        assert_eq!(scan.media[0].0, "00000127-Allergen-U\u{0308}bersicht.pdf");
    }

    #[test]
    fn local_headers_scan_keeps_utf8_names_without_flag() {
        let zip = make_whatsapp_style_zip();
        let scan = scan_zip_via_local_headers(&zip).expect("scan should recover");
        assert_eq!(scan.media[0].0, "00000127-Allergen-U\u{0308}bersicht.pdf");
    }
}
