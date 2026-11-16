mod refresh;

use std::path::PathBuf;
use std::{fs, io};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::ipc::CapabilityBuilder;
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_libmpv::MpvExt;

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
	server: Option<Url>,
	start_maximized: bool,
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
			.permission("allow-mpv-get")
			.permission("libmpv:allow-set-video-margin-ratio")
			.permission("allow-mpv-load")
			.permission("allow-mpv-seek")
			.permission("allow-mpv-stop")
			.permission("allow-mpv-set")
			.permission("allow-open-settings")
			.permission("allow-match-refresh-rate"),
	)?;
	window.navigate(url)
}

// The server's page drives mpv only through these, not through the plugin's own commands, which take any command or option, `run` included.
#[tauri::command]
fn mpv_load(app: AppHandle, url: Url, aid: Option<u32>) -> tauri_plugin_libmpv::Result<()> {
	if load_settings(&app)?.server.map(|server| server.origin()) != Some(url.origin()) {
		return Err(io::Error::other(format!("{url} is not on the configured server")).into());
	}
	let aid = aid.map_or("auto".to_string(), |aid| aid.to_string());
	app.mpv().command(
		"loadfile",
		&vec![
			json!(url),
			json!("replace"),
			json!("-1"),
			json!(format!("aid={aid}")),
		],
		"main",
	)
}

#[tauri::command]
fn mpv_seek(app: AppHandle, time: f64) -> tauri_plugin_libmpv::Result<()> {
	app.mpv()
		.command("seek", &vec![json!(time), json!("absolute")], "main")
}

#[tauri::command]
fn mpv_stop(app: AppHandle) -> tauri_plugin_libmpv::Result<()> {
	app.mpv().command("stop", &vec![], "main")
}

#[tauri::command]
fn mpv_set(app: AppHandle, name: String, value: Value) -> tauri_plugin_libmpv::Result<()> {
	if !["pause", "volume", "mute", "speed", "aid"].contains(&name.as_str()) {
		return Err(io::Error::other(format!("{name} is not settable")).into());
	}
	app.mpv().set_property(&name, &value, "main")
}

#[tauri::command]
fn mpv_get(app: AppHandle, name: String) -> tauri_plugin_libmpv::Result<Value> {
	if !["time-pos", "container-fps"].contains(&name.as_str()) {
		return Err(io::Error::other(format!("{name} is not readable")).into());
	}
	app.mpv().get_property(name, "double".to_string(), "main")
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

#[tauri::command]
async fn match_refresh_rate(app: AppHandle, fps: Option<f64>) -> tauri::Result<()> {
	let Some(fps) = fps else {
		refresh::restore();
		return sync_to_display(&app, None);
	};
	if !load_settings(&app)?.match_refresh_rate {
		return Ok(());
	}
	let window = app.get_webview_window("main").unwrap();
	let rate = refresh::switch(window.hwnd()?, fps).map_err(|e| tauri::Error::Anyhow(e.into()))?;
	sync_to_display(&app, rate)
}

/// mpv bends playback speed to the display only at a rate matched to the video, not at any rate, where it drops frames; and it reads the
/// rate when its window is created, not when the mode changes.
fn sync_to_display(app: &AppHandle, rate: Option<f64>) -> tauri::Result<()> {
	let video_sync = if rate.is_some() {
		"display-resample"
	} else {
		"audio"
	};
	for (name, value) in [
		("display-fps-override", json!(rate.unwrap_or(0.0))),
		("video-sync", json!(video_sync)),
	] {
		app.mpv()
			.set_property(name, &value, "main")
			.map_err(|e| tauri::Error::Anyhow(e.into()))?;
	}
	Ok(())
}

pub fn run() {
	tauri::Builder::default()
		.plugin(tauri_plugin_libmpv::init())
		.invoke_handler(tauri::generate_handler![
			mpv_load,
			mpv_seek,
			mpv_stop,
			mpv_set,
			mpv_get,
			get_settings,
			save_settings,
			open_settings,
			match_refresh_rate
		])
		.setup(|app| {
			let settings = load_settings(app.handle())?;
			let window =
				WebviewWindowBuilder::from_config(app.handle(), &app.config().app.windows[0])?
					.title(&app.package_info().name)
					.initialization_script(include_str!("shim.js"))
					.maximized(settings.start_maximized)
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
		.build(tauri::generate_context!())
		.expect("error while building tauri application")
		.run(|_, event| {
			if let RunEvent::Exit = event {
				refresh::restore();
			}
		});
}
