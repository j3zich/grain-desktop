const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const path = document.querySelector("#path");
const pause = document.querySelector("#pause");
const time = document.querySelector("#time");

await invoke("plugin:libmpv|init", {
	mpvConfig: {
		initialOptions: {
			vo: "gpu-next",
			hwdec: "auto-safe",
			"target-colorspace-hint": "yes",
			"keep-open": "yes",
			"force-window": "yes",
		},
		observedProperties: { pause: "flag", "time-pos": "double", duration: "double" },
	},
	windowLabel: "main",
});

let paused = false;
let position = 0;
let duration = 0;
await listen("mpv-event-main", ({ payload: { event, name, data } }) => {
	if (event !== "property-change") {
		return;
	}
	if (name === "pause") {
		paused = data;
		pause.textContent = paused ? "Play" : "Pause";
		return;
	}
	if (name === "time-pos") {
		position = data ?? 0;
	} else {
		duration = data ?? 0;
	}
	time.textContent = `${position.toFixed(1)} / ${duration.toFixed(1)} s`;
});

document.querySelector("#controls").addEventListener("submit", (e) => {
	e.preventDefault();
	invoke("plugin:libmpv|command", { name: "loadfile", args: [path.value], windowLabel: "main" });
});
pause.addEventListener("click", () => invoke("plugin:libmpv|set_property", { name: "pause", value: !paused, windowLabel: "main" }));
