const { invoke } = window.__TAURI__.core;

const url = document.querySelector("#url");
const error = document.querySelector("#error");

document.querySelector("#server").addEventListener("submit", async (e) => {
	e.preventDefault();
	const value = url.value.trim();
	const server = URL.parse(value.includes("://") ? value : `https://${value}`)?.href;
	const answers = server && (await fetch(server, { mode: "no-cors", signal: AbortSignal.timeout(10_000) }).then(() => true, () => false));
	if (!answers) {
		error.textContent = `${value} does not answer.`;
		return;
	}
	try {
		await invoke("set_server", { url: server });
	} catch (err) {
		error.textContent = String(err);
	}
});
