use std::{fs, path::Path};

use tantivy::{
    Index,
    collector::TopDocs,
    doc,
    query::QueryParser,
    schema::{Field, STORED, Schema, TEXT, TantivyDocument, Value},
};
use tauri::{AppHandle, Manager, PhysicalPosition};

#[derive(serde::Serialize)]
struct SearchResponse {
    category: String,
    answer: String,
}

struct SearchIndex {
    index: Index,
    query: Field,
    answer: Field,
    category: Field,
    fallback: SearchResponse,
}

fn build_search_index() -> Result<SearchIndex, String> {
    let mut schema_builder = Schema::builder();
    let query = schema_builder.add_text_field("query", TEXT | STORED);
    let answer = schema_builder.add_text_field("answer", STORED);
    let category = schema_builder.add_text_field("category", STORED);
    let schema = schema_builder.build();
    let index = Index::create_in_ram(schema);
    let mut writer = index
        .writer(15_000_000)
        .map_err(|error| error.to_string())?;
    let database_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("database");
    let mut fallback = None;

    for entry in fs::read_dir(&database_directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("txt") {
            continue;
        }

        let file_category = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or("Database file has no valid category name")?
            .to_string();
        let contents = fs::read_to_string(&path).map_err(|error| error.to_string())?;

        for (line_number, line) in contents.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let (prompt, response) = line.split_once('|').ok_or_else(|| {
                format!(
                    "Invalid database row in {} at line {}",
                    path.display(),
                    line_number + 1
                )
            })?;
            let prompt = prompt.trim();
            let response = response.trim();

            writer
                .add_document(doc!(
                    query => prompt,
                    answer => response,
                    category => file_category.as_str(),
                ))
                .map_err(|error| error.to_string())?;

            if file_category == "unanswerable" && fallback.is_none() {
                fallback = Some(SearchResponse {
                    category: file_category.clone(),
                    answer: response.to_string(),
                });
            }
        }
    }

    writer.commit().map_err(|error| error.to_string())?;

    Ok(SearchIndex {
        index,
        query,
        answer,
        category,
        fallback: fallback.ok_or("Database needs an unanswerable fallback row")?,
    })
}

#[tauri::command]
fn search_docs(query: String, search_index: tauri::State<'_, SearchIndex>) -> SearchResponse {
    let fallback = || SearchResponse {
        category: search_index.fallback.category.clone(),
        answer: search_index.fallback.answer.clone(),
    };
    let reader = match search_index.index.reader() {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("[Re-Clippy] could not read search index: {error}");
            return fallback();
        }
    };
    let parser = QueryParser::for_index(&search_index.index, vec![search_index.query]);
    let parsed_query = match parser.parse_query(&query) {
        Ok(parsed_query) => parsed_query,
        Err(_) => return fallback(),
    };
    let matches = match reader
        .searcher()
        .search(&parsed_query, &TopDocs::with_limit(1))
    {
        Ok(matches) => matches,
        Err(error) => {
            eprintln!("[Re-Clippy] could not search index: {error}");
            return fallback();
        }
    };
    let Some((_, address)) = matches.into_iter().next() else {
        return fallback();
    };
    let document: TantivyDocument = match reader.searcher().doc(address) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("[Re-Clippy] could not read result document: {error}");
            return fallback();
        }
    };
    let text = |field| {
        document
            .get_first(field)
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string()
    };

    SearchResponse {
        category: text(search_index.category),
        answer: text(search_index.answer),
    }
}

fn position_clippy(window: &tauri::WebviewWindow) -> Result<(), String> {
    let monitor = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .ok_or("No primary monitor found")?;

    let origin = monitor.position();
    let screen = monitor.size();
    let size = window.outer_size().map_err(|error| error.to_string())?;
    let taskbar_margin = 48_i32;

    window
        .set_position(PhysicalPosition::new(
            origin.x + screen.width as i32 - size.width as i32,
            origin.y + screen.height as i32 - size.height as i32 - taskbar_margin,
        ))
        .map_err(|error| error.to_string())
}

fn position_chat(clippy: &tauri::WebviewWindow, chat: &tauri::WebviewWindow) -> Result<(), String> {
    let clippy_position = clippy.outer_position().map_err(|error| error.to_string())?;
    let clippy_size = clippy.outer_size().map_err(|error| error.to_string())?;
    let chat_size = chat.outer_size().map_err(|error| error.to_string())?;

    let x = clippy_position.x + clippy_size.width as i32 - chat_size.width as i32;
    let y = clippy_position.y - chat_size.height as i32 - 8;

    println!(
        "[Re-Clippy] positioning chat at ({x}, {y}); Clippy at ({}, {}), size {}x{}; chat size {}x{}",
        clippy_position.x,
        clippy_position.y,
        clippy_size.width,
        clippy_size.height,
        chat_size.width,
        chat_size.height
    );

    chat.set_position(PhysicalPosition::new(x, y))
        .map_err(|error| {
            eprintln!("[Re-Clippy] could not position chat: {error}");
            error.to_string()
        })
}

#[tauri::command]
fn open_chat(app: AppHandle) -> Result<String, String> {
    println!("[Re-Clippy] open_chat command received");

    let clippy = app
        .get_webview_window("main")
        .ok_or("Clippy window not found")?;

    let chat = app
        .get_webview_window("chat")
        .ok_or("Chat window not found")?;

    if chat.is_visible().map_err(|error| error.to_string())? {
        println!("[Re-Clippy] chat already open");
        return Ok("chat already open".to_string());
    }

    println!("[Re-Clippy] positioning pre-created chat window");
    position_chat(&clippy, &chat)?;
    println!("[Re-Clippy] chat position set");
    chat.show().map_err(|error| {
        eprintln!("[Re-Clippy] could not show chat: {error}");
        error.to_string()
    })?;
    println!("[Re-Clippy] chat window shown");
    chat.set_focus().map_err(|error| {
        eprintln!("[Re-Clippy] could not focus chat: {error}");
        error.to_string()
    })?;

    Ok("chat opened".to_string())
}

#[tauri::command]
fn close_chat(app: AppHandle) -> Result<(), String> {
    println!("[Re-Clippy] close_chat command received");
    let chat = app
        .get_webview_window("chat")
        .ok_or("Chat window not found")?;

    chat.hide().map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(build_search_index().expect("could not build Re-Clippy search index"))
        .invoke_handler(tauri::generate_handler![open_chat, close_chat, search_docs])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window must exist");

            position_clippy(&window).map_err(std::io::Error::other)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Re-Clippy");
}
