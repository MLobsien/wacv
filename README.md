# WACV — WhatsApp Chat Viewer

An offline viewer for WhatsApp chat exports, built with Rust and [Dioxus](https://dioxuslabs.com) 0.7. Import a chat export (ZIP), then browse messages, media, calls and polls in a WhatsApp-style UI. Runs on Linux desktop and Android — these are also the only platforms it has been tested on.

## Features

- Import WhatsApp chat exports (ZIP), with or without media
- Chat list with last-message previews, timestamps and per-chat deletion
- Rich message rendering: text, photos/videos/audio/stickers, calls, polls, deleted-message placeholders, day separators
- Click-to-open image lightbox and clickable links
- Display-name setting for group chats, dark mode
- Media served locally by an embedded HTTP server (`~/.local/share/wacv/media`)

## ⚠️ Set WhatsApp to English

The parser relies on the localized markers WhatsApp writes into chat exports. German variants (`<Anhang: …>`, `ABSTIMMUNG:`, `Sprachanruf`, `Diese Nachricht wurde gelöscht.`) are recognized, but **English is the fully supported baseline** — polls, calls and system messages are matched most reliably in English:

| Feature | English marker |
|---|---|
| Polls | `POLL:` / `OPTION: … (N votes)` |
| Calls | `Voice call` / `Missed voice call` |
| Deleted / edited | `This message was deleted.` / `You deleted this message.` / `<This message was edited.>` |
| Media | `<Attachment: …>` |
| Encryption notice | `Messages and calls are end-to-end encrypted` |

To avoid misparsed or dropped messages, set the WhatsApp app language to **English before exporting**: *WhatsApp → Settings → Chats → App language → English*, then export the chat.

## Usage

1. Export a chat: WhatsApp → Chat → More → **Export chat** (with or without media).
2. Import the ZIP via the **Import** button (desktop opens a native file dialog, Android uses the system picker).
3. Set your display name in **Settings** so your own messages are highlighted in group chats.
4. Alternatively, import from the terminal with the bundled CLI — see below.

### CLI

```bash
wacv import <path>     # path = a .zip file, or a directory of .zip files (desktop only)
wacv --help            # show the CLI's help
```
Imports land in the same storage as the GUI, so they appear in the chat list.
## Development

```bash
dx serve                                                         # run the desktop app with hot reload
cargo test                                                       # run unit tests
npx @tailwindcss/cli -i ./input.css -o ./assets/tailwind.css     # regenerate Tailwind CSS
```

The project uses a Nix dev shell (`nix develop`) on the desktop. Tailwind is configured via `input.css` (custom dark variant) and compiled to `assets/tailwind.css`, which `dx serve` picks up automatically.
