use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use rand::seq::SliceRandom;
use tantivy::{
    Index,
    collector::TopDocs,
    doc,
    query::{BooleanQuery, BoostQuery, Occur, QueryParser, TermQuery},
    schema::{
        Field, IndexRecordOption, STORED, STRING, Schema, TEXT, TantivyDocument, Term, Value,
    },
};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition};

const TASKBAR_MARGIN: i32 = 48;
const CHAT_GAP: i32 = 8;

#[derive(serde::Serialize)]
struct SearchResponse {
    category: String,
    answer: String,
}

// Each database record can recognise several phrasings and choose from several
// replies. That lets the search stay data-driven instead of hard-coding chat
// behaviour in Rust.
#[derive(serde::Deserialize)]
struct SearchEntry {
    prompts: Vec<String>,
    responses: Vec<String>,
}

struct SearchIndex {
    index: Index,
    query: Field,
    exact_prompt: Field,
    responses: Field,
    category: Field,
    fallback_responses: Vec<String>,
}

fn normalize_prompt(prompt: &str) -> String {
    prompt
        .trim()
        .trim_end_matches(['?', '.', '!'])
        .to_lowercase()
}

fn random_response(responses: &[String]) -> String {
    responses
        .choose(&mut rand::thread_rng())
        .cloned()
        .unwrap_or_else(|| "Bing it.".to_string())
}

#[derive(serde::Serialize, Clone)]
struct ContextNudge {
    context: String,
    response: String,
}

#[derive(serde::Serialize)]
struct ActiveAppContext {
    context: String,
}

struct AppContext {
    name: String,
    applications: Vec<String>,
    responses: Vec<String>,
}

struct AppContexts(Vec<AppContext>);

struct InterruptionState {
    muted_until: Mutex<Option<Instant>>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct HistoryEntry {
    question: String,
    answer: String,
}

struct HistoryState {
    entries: Mutex<VecDeque<HistoryEntry>>,
    path: PathBuf,
}

fn build_history_state() -> Result<HistoryState, String> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("database");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("chat_history.json");
    let entries = match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => VecDeque::new(),
        Err(error) => return Err(error.to_string()),
    };

    Ok(HistoryState {
        entries: Mutex::new(entries),
        path,
    })
}

fn record_history(
    history: &HistoryState,
    question: &str,
    response: SearchResponse,
) -> SearchResponse {
    let entry = HistoryEntry {
        question: question.to_string(),
        answer: response.answer.clone(),
    };
    let save_result = (|| -> Result<(), String> {
        let mut entries = history
            .entries
            .lock()
            .map_err(|_| "History state lock was poisoned")?;
        entries.push_back(entry);
        while entries.len() > 5 {
            entries.pop_front();
        }
        let contents =
            serde_json::to_string_pretty(&*entries).map_err(|error| error.to_string())?;
        fs::write(&history.path, contents).map_err(|error| error.to_string())
    })();

    if let Err(error) = save_result {
        eprintln!("[Re-Clippy] could not save history: {error}");
    }
    response
}

fn last_question(history: &HistoryState) -> Option<String> {
    history
        .entries
        .lock()
        .ok()
        .and_then(|entries| entries.back().map(|entry| entry.question.clone()))
}

fn interruptions_are_muted(state: &InterruptionState) -> Result<bool, String> {
    let mut muted_until = state
        .muted_until
        .lock()
        .map_err(|_| "Interruption state lock was poisoned")?;
    match *muted_until {
        Some(until) if until > Instant::now() => Ok(true),
        Some(_) => {
            *muted_until = None;
            Ok(false)
        }
        None => Ok(false),
    }
}

fn build_app_contexts() -> Result<AppContexts, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("database")
        .join("app_context.txt");
    let contents = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let mut contexts = Vec::new();
    let mut current_name: Option<String> = None;
    let mut applications = Vec::new();
    let mut responses = Vec::new();

    let finish_section = |contexts: &mut Vec<AppContext>,
                          name: &mut Option<String>,
                          applications: &mut Vec<String>,
                          responses: &mut Vec<String>| {
        if let Some(name) = name.take()
            && !applications.is_empty()
            && !responses.is_empty()
        {
            contexts.push(AppContext {
                name,
                applications: std::mem::take(applications),
                responses: std::mem::take(responses),
            });
        }
    };

    for line in contents.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            finish_section(
                &mut contexts,
                &mut current_name,
                &mut applications,
                &mut responses,
            );
            current_name = Some(line[1..line.len() - 1].to_string());
            continue;
        }

        let (key, values) = line
            .split_once('=')
            .ok_or_else(|| format!("Invalid app context row: {line}"))?;
        let values = values
            .split('|')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();

        match key.trim() {
            "apps" => applications.extend(values),
            "responses" => responses.extend(values),
            other => return Err(format!("Unknown app context key: {other}")),
        }
    }
    finish_section(
        &mut contexts,
        &mut current_name,
        &mut applications,
        &mut responses,
    );

    if contexts.is_empty() {
        return Err("app_context.txt needs at least one complete section".to_string());
    }

    Ok(AppContexts(contexts))
}

fn build_search_index() -> Result<SearchIndex, String> {
    let mut schema_builder = Schema::builder();
    let query = schema_builder.add_text_field("query", TEXT);
    let exact_prompt = schema_builder.add_text_field("exact_prompt", STRING);
    let responses = schema_builder.add_text_field("responses", STORED);
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
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }

        let file_category = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or("Database file has no valid category name")?
            .to_string();
        if file_category == "chat_history" {
            continue;
        }
        let contents = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let entries: Vec<SearchEntry> = serde_json::from_str(&contents)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;

        for (entry_number, entry) in entries.into_iter().enumerate() {
            if entry.prompts.is_empty() || entry.responses.is_empty() {
                return Err(format!(
                    "Database entry {} in {} needs prompts and responses",
                    entry_number + 1,
                    path.display()
                ));
            }
            let serialized_responses =
                serde_json::to_string(&entry.responses).map_err(|error| error.to_string())?;

            for prompt in entry.prompts {
                let prompt = prompt.trim();
                if prompt.is_empty() {
                    continue;
                }
                writer
                    .add_document(doc!(
                        query => prompt,
                        exact_prompt => normalize_prompt(prompt),
                        responses => serialized_responses.as_str(),
                        category => file_category.as_str(),
                    ))
                    .map_err(|error| error.to_string())?;
            }

            if file_category == "unanswerable" && fallback.is_none() {
                fallback = Some(entry.responses);
            }
        }
    }

    writer.commit().map_err(|error| error.to_string())?;

    Ok(SearchIndex {
        index,
        query,
        exact_prompt,
        responses,
        category,
        fallback_responses: fallback.ok_or("Database needs an unanswerable fallback row")?,
    })
}

#[tauri::command]
fn search_docs(
    query: String,
    search_index: tauri::State<'_, SearchIndex>,
    history: tauri::State<'_, HistoryState>,
) -> SearchResponse {
    let normalized_query = normalize_prompt(&query);
    if matches!(
        normalized_query.as_str(),
        "what did i last ask you about" | "what was my last question" | "what did i ask last"
    ) {
        return SearchResponse {
            category: "history".to_string(),
            answer: last_question(&history)
                .map(|question| format!("You last asked me about: “{question}”"))
                .unwrap_or_else(|| "You have not asked me anything yet.".to_string()),
        };
    }

    let fallback = || SearchResponse {
        category: "unanswerable".to_string(),
        answer: random_response(&search_index.fallback_responses),
    };
    let reader = match search_index.index.reader() {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("[Re-Clippy] could not read search index: {error}");
            return record_history(&history, &query, fallback());
        }
    };
    let parser = QueryParser::for_index(&search_index.index, vec![search_index.query]);
    let loose_query = match parser.parse_query(&query) {
        Ok(parsed_query) => parsed_query,
        Err(_) => return record_history(&history, &query, fallback()),
    };
    let exact_term = Term::from_field_text(search_index.exact_prompt, &normalized_query);
    let exact_query = BoostQuery::new(
        Box::new(TermQuery::new(exact_term, IndexRecordOption::Basic)),
        10.0,
    );
    // Keep Tantivy's loose keyword search, while a whole-prompt match receives
    // a strong score boost and wins against partial overlaps.
    let ranked_query = BooleanQuery::new(vec![
        (Occur::Should, loose_query),
        (Occur::Should, Box::new(exact_query)),
    ]);
    let matches = match reader
        .searcher()
        .search(&ranked_query, &TopDocs::with_limit(1))
    {
        Ok(matches) => matches,
        Err(error) => {
            eprintln!("[Re-Clippy] could not search index: {error}");
            return record_history(&history, &query, fallback());
        }
    };
    let Some((_, address)) = matches.into_iter().next() else {
        return record_history(&history, &query, fallback());
    };
    let document: TantivyDocument = match reader.searcher().doc(address) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("[Re-Clippy] could not read result document: {error}");
            return record_history(&history, &query, fallback());
        }
    };
    let text = |field| {
        document
            .get_first(field)
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string()
    };

    record_history(
        &history,
        &query,
        SearchResponse {
            category: text(search_index.category),
            answer: serde_json::from_str::<Vec<String>>(&text(search_index.responses))
                .map(|responses| random_response(&responses))
                .unwrap_or_else(|_| {
                    "Bing it. My database filing system is having a moment.".to_string()
                }),
        },
    )
}

fn position_clippy(window: &tauri::WebviewWindow) -> Result<(), String> {
    let monitor = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .ok_or("No primary monitor found")?;

    let origin = monitor.position();
    let screen = monitor.size();
    let size = window.outer_size().map_err(|error| error.to_string())?;
    window
        .set_position(PhysicalPosition::new(
            origin.x + screen.width as i32 - size.width as i32,
            origin.y + screen.height as i32 - size.height as i32 - TASKBAR_MARGIN,
        ))
        .map_err(|error| error.to_string())
}

fn position_chat(clippy: &tauri::WebviewWindow, chat: &tauri::WebviewWindow) -> Result<(), String> {
    let monitor = clippy
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .ok_or("No primary monitor found")?;
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let clippy_position = clippy.outer_position().map_err(|error| error.to_string())?;
    let clippy_size = clippy.outer_size().map_err(|error| error.to_string())?;
    let chat_size = chat.outer_size().map_err(|error| error.to_string())?;

    let preferred_x = clippy_position.x + clippy_size.width as i32 - chat_size.width as i32;
    let preferred_y = clippy_position.y - chat_size.height as i32 - CHAT_GAP;
    let min_x = monitor_position.x;
    let max_x = monitor_position.x + monitor_size.width as i32 - chat_size.width as i32;
    let min_y = monitor_position.y + CHAT_GAP;
    let max_y =
        monitor_position.y + monitor_size.height as i32 - chat_size.height as i32 - TASKBAR_MARGIN;
    let x = preferred_x.clamp(min_x, max_x);
    let y = preferred_y.clamp(min_y, max_y);

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

#[cfg(target_os = "windows")]
fn active_window_title() -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

    // Windows owns this buffer; we only read the title of its foreground window.
    let mut title_buffer = [0u16; 512];
    let title_length = unsafe { GetWindowTextW(GetForegroundWindow(), &mut title_buffer) };
    String::from_utf16_lossy(&title_buffer[..title_length.max(0) as usize]).to_lowercase()
}

#[cfg(not(target_os = "windows"))]
fn active_window_title() -> String {
    String::new()
}

#[tauri::command]
fn active_app_context(contexts: tauri::State<'_, AppContexts>) -> Option<ActiveAppContext> {
    let window_title = active_window_title();

    contexts.0.iter().find_map(|context| {
        let matches = context
            .applications
            .iter()
            .any(|application| window_title.contains(application));
        if !matches {
            return None;
        }

        Some(ActiveAppContext {
            context: context.name.clone(),
        })
    })
}

#[tauri::command]
fn show_context_nudge(
    context: String,
    contexts: tauri::State<'_, AppContexts>,
    interruption_state: tauri::State<'_, InterruptionState>,
    app: AppHandle,
) -> Result<(), String> {
    if interruptions_are_muted(&interruption_state)? {
        println!("[Re-Clippy] context nudge skipped during quiet time");
        return Ok(());
    }

    let app_context = contexts
        .0
        .iter()
        .find(|app_context| app_context.name == context)
        .ok_or("App context not found")?;
    let response = app_context
        .responses
        .choose(&mut rand::thread_rng())
        .ok_or("App context has no responses")?
        .clone();
    let nudge = ContextNudge {
        context: app_context.name.clone(),
        response,
    };

    println!("[Re-Clippy] {} activity detected", nudge.context);
    let clippy = app
        .get_webview_window("main")
        .ok_or("Clippy window not found")?;
    let chat = app
        .get_webview_window("chat")
        .ok_or("Chat window not found")?;

    if clippy.is_minimized().map_err(|error| error.to_string())? {
        println!("[Re-Clippy] context nudge skipped while Clippy is minimized");
        return Ok(());
    }
    position_clippy(&clippy)?;

    if !chat.is_visible().map_err(|error| error.to_string())? {
        position_chat(&clippy, &chat)?;
        chat.show().map_err(|error| error.to_string())?;
    }

    app.emit("app-context-detected", nudge)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn snooze_interruptions(
    interruption_state: tauri::State<'_, InterruptionState>,
) -> Result<(), String> {
    let mut muted_until = interruption_state
        .muted_until
        .lock()
        .map_err(|_| "Interruption state lock was poisoned")?;
    *muted_until = Some(Instant::now() + Duration::from_secs(15 * 60));
    println!("[Re-Clippy] interruptions muted for 15 minutes");
    Ok(())
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

    position_clippy(&clippy)?;
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

#[tauri::command]
fn resize_chat(height: f64, app: AppHandle) -> Result<(), String> {
    let clippy = app
        .get_webview_window("main")
        .ok_or("Clippy window not found")?;
    let chat = app
        .get_webview_window("chat")
        .ok_or("Chat window not found")?;

    // The frontend measures CSS pixels. Tauri's LogicalSize keeps this correct
    // when Windows display scaling is enabled.
    let height = height.clamp(120.0, 600.0);
    chat.set_size(LogicalSize::new(360.0, height))
        .map_err(|error| error.to_string())?;
    position_chat(&clippy, &chat)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(build_search_index().expect("could not build Re-Clippy search index"))
        .manage(build_app_contexts().expect("could not load Re-Clippy app contexts"))
        .manage(InterruptionState {
            muted_until: Mutex::new(None),
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            open_chat,
            close_chat,
            resize_chat,
            search_docs,
            active_app_context,
            show_context_nudge,
            snooze_interruptions
        ])
        .setup(|app| {
            let _ = app.manage(build_history_state().map_err(std::io::Error::other)?);
            let window = app
                .get_webview_window("main")
                .expect("main window must exist");

            position_clippy(&window).map_err(std::io::Error::other)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Re-Clippy");
}
