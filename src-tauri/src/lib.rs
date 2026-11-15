use std::fs;
use std::path::PathBuf;

use serde_json::json;
use tauri::ipc::CapabilityBuilder;
use tauri::{AppHandle, Manager, Url, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_libmpv::MpvExt;

fn server_file(app: &AppHandle) -> tauri::Result<PathBuf> {
	Ok(app.path().app_config_dir()?.join("server.txt"))
}

fn open_server(window: &WebviewWindow, url: Url) -> tauri::Result<()> {
	window.add_capability(
		CapabilityBuilder::new("server")
			.remote(format!("{}/*", url.origin().ascii_serialization()))
			.window(window.label())
			.permission("core:event:default")
			.permission("libmpv:default"),
	)?;
	window.navigate(url)
}

#[tauri::command]
fn set_server(app: AppHandle, url: Url) -> tauri::Result<()> {
	let file = server_file(&app)?;
	fs::create_dir_all(file.parent().unwrap())?;
	fs::write(&file, url.as_str())?;
	open_server(&app.get_webview_window("main").unwrap(), url)
}

pub fn run() {
	tauri::Builder::default()
		.plugin(tauri_plugin_libmpv::init())
		.invoke_handler(tauri::generate_handler![set_server])
		.setup(|app| {
			let window =
				WebviewWindowBuilder::from_config(app.handle(), &app.config().app.windows[0])?
					.initialization_script(include_str!("shim.js"))
					.visible(false)
					.build()?;
			// mpv's window stays black behind the transparent webview, so the window never shows the desktop through it.
			app.mpv().init(
				serde_json::from_value(json!({
					"initialOptions": {
						"vo": "gpu-next",
						"hwdec": "auto-safe",
						"target-colorspace-hint": "yes",
						"force-window": "yes",
						"keep-open": "yes",
						// Silo reads the clock at loadedmetadata as the container's start time, as a browser reports it.
						"rebase-start-time": "no",
						// Silo draws the subtitles.
						"sid": "no",
					},
					"observedProperties": {
						"pause": "flag",
						"time-pos": "double",
						"duration": "double",
						"paused-for-cache": "flag",
						"demuxer-cache-time": "double",
						"eof-reached": "flag",
						"width": "int64",
						"height": "int64",
						"volume": "double",
						"mute": "flag",
						"speed": "double",
					},
				}))?,
				window.label(),
			)?;
			window.show()?;
			let Ok(saved) = fs::read_to_string(server_file(app.handle())?) else {
				return Ok(());
			};
			open_server(&window, Url::parse(&saved)?)?;
			Ok(())
		})
		.run(tauri::generate_context!())
		.expect("error while running tauri application");
}
