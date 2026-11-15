use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::ipc::CapabilityBuilder;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_libmpv::MpvExt;

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
	server: Option<Url>,
	match_refresh_rate: bool,
	passthrough: Vec<String>,
	exclusive_audio: bool,
	audio_channels: String,
}

impl Settings {
	fn mpv_options(&self) -> [(&'static str, Value); 3] {
		[
			("audio-spdif", json!(self.passthrough.join(","))),
			("audio-exclusive", json!(self.exclusive_audio)),
			(
				"audio-channels",
				json!(if self.audio_channels.is_empty() {
					"auto-safe"
				} else {
					self.audio_channels.as_str()
				}),
			),
		]
	}
}

fn settings_file(app: &AppHandle) -> tauri::Result<PathBuf> {
	Ok(app.path().app_local_data_dir()?.join("settings.json"))
}

fn load_settings(app: &AppHandle) -> tauri::Result<Settings> {
	let Ok(text) = fs::read_to_string(settings_file(app)?) else {
		return Ok(Settings::default());
	};
	Ok(serde_json::from_str(&text)?)
}

fn open_server(window: &WebviewWindow, url: Url) -> tauri::Result<()> {
	window.add_capability(
		CapabilityBuilder::new("server")
			.remote(format!("{}/*", url.origin().ascii_serialization()))
			.window(window.label())
			.permission("core:event:default")
			.permission("libmpv:default")
			.permission("allow-open-settings"),
	)?;
	window.navigate(url)
}

#[tauri::command]
fn get_settings(app: AppHandle) -> tauri::Result<Settings> {
	load_settings(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, window: WebviewWindow, settings: Settings) -> tauri::Result<()> {
	let previous = load_settings(&app)?;
	let file = settings_file(&app)?;
	fs::create_dir_all(file.parent().unwrap())?;
	fs::write(&file, serde_json::to_string_pretty(&settings)?)?;
	for (name, value) in settings.mpv_options() {
		app.mpv()
			.set_property(name, &value, "main")
			.map_err(|e| tauri::Error::Anyhow(e.into()))?;
	}
	let main = app.get_webview_window("main").unwrap();
	if let Some(server) = settings
		.server
		.filter(|server| window.label() == "main" || previous.server.as_ref() != Some(server))
	{
		open_server(&main, server)?;
	}
	if window.label() == "settings" {
		window.close()?;
	}
	Ok(())
}

#[tauri::command]
async fn open_settings(app: AppHandle) -> tauri::Result<()> {
	if let Some(window) = app.get_webview_window("settings") {
		return window.set_focus();
	}
	let mut builder = WebviewWindowBuilder::new(&app, "settings", WebviewUrl::default())
		.title("Settings")
		.inner_size(440.0, 440.0)
		.resizable(false)
		.maximizable(false);
	// WebView2 refuses a second window whose browser arguments differ from the main window's.
	if let Some(args) = &app.config().app.windows[0].additional_browser_args {
		builder = builder.additional_browser_args(args);
	}
	builder.build()?;
	Ok(())
}

pub fn run() {
	tauri::Builder::default()
		.plugin(tauri_plugin_libmpv::init())
		.invoke_handler(tauri::generate_handler![
			get_settings,
			save_settings,
			open_settings
		])
		.setup(|app| {
			let settings = load_settings(app.handle())?;
			let window =
				WebviewWindowBuilder::from_config(app.handle(), &app.config().app.windows[0])?
					.initialization_script(include_str!("shim.js"))
					.visible(false)
					.build()?;
			let mut options = json!({
				"vo": "gpu-next",
				"hwdec": "auto-safe",
				"target-colorspace-hint": "yes",
				"force-window": "yes",
				"keep-open": "yes",
				// Silo reads the clock at loadedmetadata as the container's start time, as a browser reports it.
				"rebase-start-time": "no",
				// Silo draws the subtitles.
				"sid": "no",
			});
			for (name, value) in settings.mpv_options() {
				options[name] = value;
			}
			options["config"] = json!("yes");
			options["config-dir"] = json!(app.path().app_local_data_dir()?);
			// mpv's window stays black behind the transparent webview, so the window never shows the desktop through it.
			app.mpv().init(
				serde_json::from_value(json!({
					"initialOptions": options,
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
			if let Some(server) = settings.server {
				open_server(&window, server)?;
			}
			Ok(())
		})
		.run(tauri::generate_context!())
		.expect("error while running tauri application");
}
