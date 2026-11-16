fn main() {
	tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
		tauri_build::AppManifest::new().commands(&[
			"mpv_load",
			"mpv_seek",
			"mpv_stop",
			"mpv_set",
			"mpv_get",
			"get_settings",
			"save_settings",
			"open_settings",
			"match_refresh_rate",
		]),
	))
	.expect("failed to run tauri-build");
}
