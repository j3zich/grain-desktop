(() => {
	localStorage.setItem("silo-player-browser-decoding", "false");

	const containers = ["mkv", "mp4", "webm", "avi", "ts", "m2ts", "wmv", "flv", "ogg", "mp3", "flac"];
	const videoCodecs = ["h264", "hevc", "av1", "vp9", "vp8", "mpeg2video", "mpeg4", "vc1"];
	const audioCodecs = [
		"aac", "mp3", "mp2", "opus", "vorbis", "flac", "alac", "ac3", "eac3", "dts", "truehd", "mlp",
		"pcm_s16le", "pcm_s16be", "pcm_s24le", "pcm_s24be", "pcm_s32le", "pcm_s32be", "pcm_f32le", "pcm_f64le",
	];
	const planning = /^\/api\/v2\/playback\/(start|[^/]+\/replan)$/;

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

	const fetch = window.fetch;
	window.fetch = function (input, init) {
		const url = new URL(input instanceof Request ? input.url : input, location.href);
		if (!planning.test(url.pathname) || typeof init?.body !== "string") {
			return fetch.call(this, input, init);
		}
		return fetch.call(this, input, { ...init, body: declareMpv(init.body) });
	};
})();
