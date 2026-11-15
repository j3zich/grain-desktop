use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Manager, Url};

fn server_file(app: &AppHandle) -> tauri::Result<PathBuf> {
	Ok(app.path().app_config_dir()?.join("server.txt"))
}

#[tauri::command]
fn set_server(app: AppHandle, url: Url) -> tauri::Result<()> {
	let file = server_file(&app)?;
	fs::create_dir_all(file.parent().unwrap())?;
	fs::write(&file, url.as_str())?;
	app.get_webview_window("main").unwrap().navigate(url)
}

pub fn run() {
	tauri::Builder::default()
		.plugin(tauri_plugin_libmpv::init())
		.invoke_handler(tauri::generate_handler![set_server])
		.setup(|app| {
			let Ok(saved) = fs::read_to_string(server_file(app.handle())?) else {
				return Ok(());
			};
			app.get_webview_window("main")
				.unwrap()
				.navigate(Url::parse(&saved)?)?;
			Ok(())
		})
		.run(tauri::generate_context!())
		.expect("error while running tauri application");
}
