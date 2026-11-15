fn main() {
	tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
		tauri_build::AppManifest::new().commands(&[
			"get_settings",
			"save_settings",
			"open_settings",
			"match_refresh_rate",
		]),
	))
	.expect("failed to run tauri-build");
}
