const { invoke } = window.__TAURI__.core;

const form = document.querySelector("form");
const error = document.querySelector("#error");

const settings = await invoke("get_settings");
form.elements.server.value = settings.server ?? "";
form.elements.startMaximized.checked = settings.startMaximized;
form.elements.matchRefreshRate.checked = settings.matchRefreshRate;
form.elements.exclusiveAudio.checked = settings.exclusiveAudio;
form.elements.audioChannels.value = settings.audioChannels;
for (const input of form.querySelectorAll("[name=passthrough]")) {
	input.checked = settings.passthrough.includes(input.value);
}

form.addEventListener("submit", async (e) => {
	e.preventDefault();
	const data = new FormData(form);
	const value = String(data.get("server")).trim();
	const server = URL.parse(value.includes("://") ? value : `https://${value}`)?.href;
	const answers = server && (await fetch(server, { mode: "no-cors", signal: AbortSignal.timeout(10_000) }).then(() => true, () => false));
	if (!answers) {
		error.textContent = `${value} does not answer.`;
		return;
	}
	try {
		await invoke("save_settings", {
			settings: {
				server,
				startMaximized: data.has("startMaximized"),
				matchRefreshRate: data.has("matchRefreshRate"),
				passthrough: data.getAll("passthrough"),
				exclusiveAudio: data.has("exclusiveAudio"),
				audioChannels: data.get("audioChannels"),
			},
		});
	} catch (err) {
		error.textContent = String(err);
	}
});
