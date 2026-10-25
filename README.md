# Re-Clippy

Re-Clippy is a playful revival of the classic desktop assistant. Instead of building just another AI chat wrapper, I wanted to revitalize the annoyance that was Clippy. Rather than being a forgotten paperclip from the past, Clippy lives in the bottom-right corner of the Windows desktop and acts like an intrusive ~~and useful~~ companion.

Click Clippy to open a speech-bubble chat window. Ask a Windows question such as “where are my screenshots?” and Clippy gives a practical answer. Ask a stranger question, and expect a snarky response instead. It can also do a 360° flip when asked nicely.

## How it works

Re-Clippy is a native desktop application built with Rust, Tauri, and Svelte.

Its answers are stored locally in structured JSON knowledge-base files. Every topic includes multiple natural ways a user might phrase a question and several possible replies. At startup, the Rust backend turns these prompt aliases into a Tantivy full-text search index.

Tantivy gives exact prompt matches a strong score boost over loose keyword matches. This means a direct question gets the best answer first, while similar phrasing can still find the right topic. Once a topic is selected, Clippy randomly picks from its reply pool, so it sounds less repetitive.

Re-Clippy also checks the title of the active Windows application. When it recognises an IDE, document editor, presentation app, game launcher, or chat app, it can show a context-aware message. For example, when Visual Studio Code is open, Clippy might remind you that it “knows a guy named Co-Pilot.” Users can silence these interruptions for 15 minutes.

The app keeps the last five questions and replies in a local JSON file, so asking “what did I last ask you about?” feels more like talking to a continuing desktop companion than using a one-off search box.

## What it looks like

Re-Clippy is intentionally small and nostalgic:

- Clippy stays fixed just above the taskbar in the bottom-right corner.
- Clicking it opens a pale-yellow rounded speech bubble above the character.
- The paperclip gently hovers when idle and spins when asked to “do a flip.”
- The chat interface uses a minimal input, a close button, a 15-minute quiet-mode button, and an arrow to submit a question.

## Tech stack

- **Rust** for the desktop backend, local file handling, search, window positioning, and Windows app-context detection
- **Tauri** for the lightweight native desktop shell
- **Svelte** for the chat interface and animations
- **Tantivy** for local full-text search and exact-match ranking boosts
- **Serde and JSON** for the local knowledge base and recent conversation history
- **Windows API** for foreground-window detection

## Privacy

Re-Clippy is local-first. Questions, the searchable knowledge base, recent chat history, and foreground-app checks stay on the computer; no server or external AI API is required.

## Image credit

The Clippy image is sourced from [Base-Breaking Character Wiki](https://static.wikia.nocookie.net/base-breaking-character/images/c/cb/Clippy.png/revision/latest?cb=20250920160213).

## Build

```powershell
npm install
npm run tauri:dev
```

`tauri:dev` intentionally uses `--no-watch`: chat history is written to the local database, and a normal watcher would restart the application whenever that file changes.
