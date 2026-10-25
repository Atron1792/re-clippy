use tauri::{Manager, PhysicalPosition};

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
