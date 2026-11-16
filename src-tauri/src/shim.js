(() => {
	localStorage.setItem("silo-player-browser-decoding", "false");

	const containers = ["mkv", "mp4", "webm", "avi", "ts", "m2ts", "wmv", "flv", "ogg", "mp3", "flac"];
	const videoCodecs = ["h264", "hevc", "av1", "vp9", "vp8", "mpeg2video", "mpeg4", "vc1"];
	const audioCodecs = [
		"aac", "mp3", "mp2", "opus", "vorbis", "flac", "alac", "ac3", "eac3", "dts", "truehd", "mlp",
		"pcm_s16le", "pcm_s16be", "pcm_s24le", "pcm_s24be", "pcm_s32le", "pcm_s32be", "pcm_f32le", "pcm_f64le",
	];
	const planning = /^\/api\/v2\/playback\/(start|[^/]+\/replan)$/;
	const stream = /^(\/api\/v2\/stream\/[^/]+|.*\/stream\/(direct|remux)\/.+)$/;

	// mpv plays the untouched original and manages the dynamic range itself, so plan requests declare mpv, not what the browser probes.
	function declareMpv(body) {
		const request = JSON.parse(body);
		const original = request.client_playback_context?.deliveries?.original_http;
		if (!request.client_capabilities || !original) {
			return body;
		}
		Object.assign(request.client_capabilities, {
			containers,
			codecs_video: videoCodecs,
			codecs_video_hardware: videoCodecs,
			codecs_audio: audioCodecs,
		});
		Object.assign(original, {
			enabled: true,
			supported_on_device: true,
			containers,
			video_codecs: videoCodecs,
			audio_decode_codecs: audioCodecs,
			validated_claims: ["client_managed_dynamic_range_v1", "client_selected_audio_track_v1"],
		});
		return JSON.stringify(request);
	}

	let audioOrdinal = null;
	const fetch = window.fetch;
	window.fetch = function (input, init) {
		const url = new URL(input instanceof Request ? input.url : input, location.href);
		if (!planning.test(url.pathname) || typeof init?.body !== "string") {
			return fetch.call(this, input, init);
		}
		const response = fetch.call(this, input, { ...init, body: declareMpv(init.body) });
		response.then((r) => r.clone().json()).then((r) => {
			audioOrdinal = r.playback_plan?.selected_tracks?.audio?.index ?? null;
		}, () => {});
		return response;
	};

	const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(`plugin:libmpv|${command}`, { ...args, windowLabel: "main" });
	const setProperty = (name, value) => window.__TAURI_INTERNALS__.invoke("mpv_set", { name, value });
	let listening = null;

	class Track extends EventTarget {
		mode = "disabled";
		list = [];
		active = [];

		constructor(kind, label, language) {
			super();
			Object.assign(this, { kind, label, language });
		}

		get cues() {
			return this.mode === "disabled" ? null : this.list;
		}

		// Computed from the clock, not kept from the last tick: Silo reads it itself while paused, when no tick comes.
		get activeCues() {
			return this.mode === "disabled" ? null : this.list.filter((cue) => cue.startTime <= state.time && state.time < cue.endTime);
		}

		addCue(cue) {
			this.list.push(cue);
		}

		removeCue(cue) {
			this.list = this.list.filter((c) => c !== cue);
		}

		update() {
			const next = this.activeCues ?? [];
			if (next.length === this.active.length && next.every((cue, i) => cue === this.active[i])) {
				return;
			}
			this.active = next;
			this.dispatchEvent(new Event("cuechange"));
		}
	}

	const ranges = (start, end) => ({ length: end > start ? 1 : 0, start: () => start, end: () => end });

	let video = null;
	let tracks = [];
	let backgrounds = [];
	// The browser paints the fullscreen element's backdrop black, over mpv.
	const backdrop = document.createElement("style");
	backdrop.textContent = "::backdrop { background: transparent !important; }";
	let lastTick = 0;
	let frames = 0;
	const state = { volume: 1, muted: false, speed: 1 };

	const fire = (...types) => types.forEach((type) => video?.dispatchEvent(new Event(type)));

	// mpv draws over the whole window; the margins keep its picture inside the element, which Silo shrinks for the post-roll screen.
	let margins = "";
	function fit() {
		const rect = video.getBoundingClientRect();
		const ratio = (value) => Math.min(1, Math.max(0, value));
		const next = {
			left: ratio(rect.left / innerWidth),
			top: ratio(rect.top / innerHeight),
			right: ratio(1 - rect.right / innerWidth),
			bottom: ratio(1 - rect.bottom / innerHeight),
		};
		if (JSON.stringify(next) !== margins) {
			margins = JSON.stringify(next);
			invoke("set_video_margin_ratio", { ratio: next });
		}
		uncover(rect);
		round();
	}

	// mpv's picture is square under a rounded element, so an outline in mpv's black covers its corners where the browser would clip a <video>.
	let rounded = null;
	function round() {
		let el = video;
		while (el && !parseFloat(getComputedStyle(el).borderTopLeftRadius)) {
			el = el.parentElement;
		}
		if (rounded?.[0] === el) {
			return;
		}
		unround();
		if (!el) {
			return;
		}
		rounded = [el, el.style.getPropertyValue("outline"), el.style.getPropertyPriority("outline")];
		el.style.setProperty("outline", `${getComputedStyle(el).borderTopLeftRadius} solid black`, "important");
	}

	function unround() {
		if (!rounded) {
			return;
		}
		const [el, value, priority] = rounded;
		el.style.setProperty("outline", value, priority);
		rounded = null;
	}

	// mpv shows only where nothing in the page paints, so what paints under the element (the post-roll screen's backdrop) is hidden, not what lies above it.
	let hidden = [];
	function uncover(rect) {
		const stack = document.elementsFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
		const at = stack.indexOf(video);
		if (at < 0) {
			return;
		}
		for (const el of stack.slice(at + 1)) {
			const style = getComputedStyle(el);
			const paints = style.backgroundColor !== "rgba(0, 0, 0, 0)" || style.backgroundImage !== "none" || el instanceof HTMLImageElement || el instanceof HTMLCanvasElement;
			if (!paints || el.contains(video) || hidden.some(([h]) => h === el)) {
				continue;
			}
			hidden.push([el, el.style.getPropertyValue("visibility"), el.style.getPropertyPriority("visibility")]);
			el.style.setProperty("visibility", "hidden", "important");
		}
	}

	let frame = 0;
	const refit = () => {
		frame ||= requestAnimationFrame(() => {
			frame = 0;
			if (video) {
				fit();
			}
		});
	};
	const resized = new ResizeObserver(refit);
	const mutated = new MutationObserver(refit);

	function seek(time) {
		Object.assign(state, { time, seeking: true, ended: false });
		fire("seeking", "timeupdate");
		window.__TAURI_INTERNALS__.invoke("mpv_seek", { time });
	}

	const facade = {
		src: {
			get: () => state.src,
			set: (src) => {
				const element = video;
				stop();
				element.src = src;
			},
		},
		currentSrc: { get: () => state.src },
		currentTime: { get: () => state.time, set: seek },
		duration: { get: () => state.duration },
		paused: { get: () => state.paused },
		ended: { get: () => state.ended },
		seeking: { get: () => state.seeking },
		readyState: { get: () => (!state.loaded ? 0 : state.seeking || state.buffering ? 2 : 4) },
		networkState: { get: () => (state.loaded ? 1 : 2) },
		buffered: { get: () => ranges(state.time, Math.max(state.time, state.cached)) },
		seekable: { get: () => ranges(0, state.duration || 0) },
		volume: { get: () => state.volume, set: (volume) => setProperty("volume", volume * 100) },
		muted: { get: () => state.muted, set: (muted) => setProperty("mute", muted) },
		playbackRate: { get: () => state.speed, set: (speed) => setProperty("speed", speed) },
		videoWidth: { get: () => state.width },
		videoHeight: { get: () => state.height },
		error: { get: () => state.error },
		play: {
			value: () => {
				if (state.ended) {
					seek(0);
				}
				setProperty("pause", false);
				return Promise.resolve();
			},
		},
		pause: { value: () => void setProperty("pause", true) },
		load: { value: stop },
		addTextTrack: {
			value: (kind, label = "", language = "") => {
				const track = new Track(kind, label, language);
				tracks.push(track);
				return track;
			},
		},
		requestVideoFrameCallback: {
			value: (callback) => requestAnimationFrame((now) => callback(now, {
				mediaTime: state.time,
				presentationTime: now,
				expectedDisplayTime: now,
				width: state.width,
				height: state.height,
				presentedFrames: ++frames,
			})),
		},
		cancelVideoFrameCallback: { value: (id) => cancelAnimationFrame(id) },
		getVideoPlaybackQuality: { value: () => ({ droppedVideoFrames: 0, totalVideoFrames: frames, creationTime: performance.now() }) },
		requestPictureInPicture: { value: () => Promise.reject(new DOMException("Picture-in-picture is unavailable.", "NotSupportedError")) },
	};

	function attach(element) {
		video = element;
		Object.assign(state, { volume: element.volume, muted: element.muted });
		for (const [name, descriptor] of Object.entries(facade)) {
			Object.defineProperty(element, name, "value" in descriptor ? { configurable: true, writable: true, ...descriptor } : { configurable: true, ...descriptor });
		}
		for (let el = element; el; el = el.parentElement) {
			backgrounds.push([el, el.style.getPropertyValue("background"), el.style.getPropertyPriority("background")]);
			el.style.setProperty("background", "transparent", "important");
		}
		document.head.append(backdrop);
		resized.observe(element);
		mutated.observe(document.body, { childList: true, subtree: true });
		addEventListener("resize", refit);
	}

	function detach() {
		for (const name of Object.keys(facade)) {
			delete video[name];
		}
		Object.assign(video, { volume: state.volume, muted: state.muted });
		for (const [el, value, priority] of backgrounds) {
			el.style.setProperty("background", value, priority);
		}
		Object.assign(state, { src: "", loaded: false });
		backgrounds = [];
		backdrop.remove();
		resized.disconnect();
		mutated.disconnect();
		removeEventListener("resize", refit);
		margins = "";
		invoke("set_video_margin_ratio", { ratio: { left: 0, top: 0, right: 0, bottom: 0 } });
		for (const [el, value, priority] of hidden) {
			el.style.setProperty("visibility", value, priority);
		}
		hidden = [];
		unround();
		tracks = [];
		video = null;
	}

	async function load(element, src) {
		if (video !== element) {
			if (video) {
				stop();
			}
			attach(element);
		}
		const reused = kept?.path === new URL(src, location.href).pathname ? kept : null;
		kept = null;
		Object.assign(state, {
			src: new URL(src, location.href).href,
			loaded: false,
			time: 0,
			duration: NaN,
			paused: true,
			seeking: false,
			buffering: false,
			cached: 0,
			ended: false,
			width: 0,
			height: 0,
			error: null,
		});
		fire("emptied", "loadstart");
		listening ??= window.__TAURI__.event.listen("mpv-event-main", ({ payload }) => onEvent(payload));
		await listening;
		await setProperty("pause", true);
		await setProperty("volume", state.volume * 100);
		await setProperty("mute", state.muted);
		const aid = audioOrdinal === null ? null : audioOrdinal + 1;
		if (reused) {
			const { path, origin, ...media } = reused;
			// mpv refuses a number for aid, which the plugin passes as a double.
			await setProperty("aid", String(aid ?? "auto"));
			Object.assign(state, media);
			opened(origin);
			return;
		}
		await window.__TAURI_INTERNALS__.invoke("mpv_load", { url: state.src, aid });
	}

	function opened(time) {
		Object.assign(state, { time, origin: time, loaded: true });
		fire("durationchange", "loadedmetadata", "resize", "loadeddata", "canplay", "canplaythrough");
		matchRefreshRate();
	}

	// A replan tears the element down and sets its new src in the same task, so mpv stops a task later, not at once; a new src of the same file
	// then only switches the audio track instead of reopening the file.
	let kept = null;
	function stop() {
		if (!video) {
			return;
		}
		if (state.loaded && !state.error) {
			const { origin, duration, width, height } = state;
			kept = { path: new URL(state.src).pathname, origin, duration, width, height };
			setTimeout(release);
		} else {
			window.__TAURI_INTERNALS__.invoke("mpv_stop");
		}
		detach();
	}

	function release() {
		if (kept) {
			kept = null;
			window.__TAURI_INTERNALS__.invoke("mpv_stop");
		}
	}

	async function onEvent(event) {
		if (!video) {
			return;
		}
		if (event.event === "file-loaded") {
			opened((await window.__TAURI_INTERNALS__.invoke("mpv_get", { name: "time-pos" }).catch(() => null)) ?? 0);
			return;
		}
		if (event.event === "playback-restart") {
			if (!state.seeking) {
				return;
			}
			state.seeking = false;
			tracks.forEach((track) => track.update());
			fire("seeked", "timeupdate", "canplay");
			if (!state.paused) {
				fire("playing");
			}
			return;
		}
		if (event.event === "end-file" && event.reason === "error") {
			state.error = { code: 4, message: String(event.error ?? "mpv could not play the file") };
			fire("error");
			return;
		}
		if (event.event !== "property-change") {
			return;
		}
		const data = event.data;
		switch (event.name) {
			case "time-pos":
				// mpv plays on while a reused file is not reported loaded yet; Silo reads the clock's start at loadedmetadata.
				if (state.seeking || data == null || !state.loaded) {
					return;
				}
				state.time = data;
				tracks.forEach((track) => track.update());
				if (performance.now() - lastTick < 250) {
					return;
				}
				lastTick = performance.now();
				fire("timeupdate", "progress");
				return;
			case "pause":
				// A source change pauses without a pause event, as the browser's load algorithm does.
				if (data === state.paused) {
					return;
				}
				state.paused = data;
				fire(data ? "pause" : "play");
				if (!data && state.loaded) {
					fire("playing");
				}
				return;
			case "duration":
				state.duration = data ?? NaN;
				fire("durationchange");
				return;
			case "paused-for-cache":
				state.buffering = data;
				if (!state.paused) {
					fire(data ? "waiting" : "playing");
				}
				return;
			case "demuxer-cache-time":
				state.cached = data ?? 0;
				return;
			case "eof-reached":
				if (data) {
					state.ended = true;
					fire("ended");
				}
				return;
			case "width":
			case "height":
				state[event.name] = data ?? 0;
				fire("resize");
				return;
			case "volume":
				state.volume = data / 100;
				fire("volumechange");
				return;
			case "mute":
				state.muted = data;
				fire("volumechange");
				return;
			case "speed":
				state.speed = data;
				fire("ratechange");
				return;
		}
	}

	const nativeSrc = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, "src");
	Object.defineProperty(HTMLMediaElement.prototype, "src", {
		...nativeSrc,
		set(src) {
			if (!(this instanceof HTMLVideoElement) || !stream.test(new URL(src, location.href).pathname)) {
				nativeSrc.set.call(this, src);
				return;
			}
			load(this, src);
		},
	});

	// Only in fullscreen, not in a window: switching the display's mode makes the picture lag.
	async function matchRefreshRate() {
		const fps = video && state.loaded && document.fullscreenElement
			? await window.__TAURI_INTERNALS__.invoke("mpv_get", { name: "container-fps" }).catch(() => null)
			: null;
		window.__TAURI_INTERNALS__.invoke("match_refresh_rate", { fps });
	}

	addEventListener("fullscreenchange", matchRefreshRate);
	addEventListener("pagehide", () => {
		stop();
		release();
		matchRefreshRate();
	});
	addEventListener("keydown", (e) => {
		if (e.ctrlKey && e.key === ",") {
			window.__TAURI_INTERNALS__.invoke("open_settings");
		}
	});

	// Silo's own engine, hls.js or a stream URL the shim does not know play without mpv, and nothing else would show it.
	const badge = document.createElement("div");
	badge.textContent = "Playing in the browser, not mpv";
	badge.style.cssText = "position: fixed; top: 12px; right: 12px; z-index: 2147483647; padding: 4px 10px; border-radius: 6px;"
		+ " background: #b91c1c; color: white; font: 13px system-ui, sans-serif; pointer-events: none;";
	setInterval(() => {
		if ([...document.querySelectorAll("video")].some((v) => v !== video && !v.paused)) {
			document.body?.append(badge);
			return;
		}
		badge.remove();
	}, 1000);
})();
