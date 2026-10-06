// T038: for each video, check canPlayType, load via asset protocol, seek to 30 s (or mid-point),
// and time the seek. Results go back to Rust, which prints them and exits.
const { invoke, convertFileSrc } = window.__TAURI__.core;
const log = (m) => (document.getElementById("log").textContent += m + "\n");

function once(el, events, ms) {
  return new Promise((resolve) => {
    const t = setTimeout(() => done("timeout"), ms);
    const handlers = {};
    function done(name) {
      clearTimeout(t);
      for (const e of events) el.removeEventListener(e, handlers[e]);
      resolve(name);
    }
    for (const e of events) {
      handlers[e] = () => done(e);
      el.addEventListener(e, handlers[e]);
    }
  });
}

async function rangeFetch(url) {
  try {
    const res = await fetch(url, { headers: { Range: "bytes=0-99" } });
    const body = await res.arrayBuffer();
    return { status: res.status, type: res.headers.get("content-type"),
             range: res.headers.get("content-range"), accept: res.headers.get("accept-ranges"), bytes: body.byteLength };
  } catch (e) { return { fetch_error: String(e) }; }
}

async function probe(v, path, viaBlob, url) {
  const r = { path, url: url || convertFileSrc(path), via: viaBlob ? "blob" : (url ? "http" : "asset") };
  r.fetch = await rangeFetch(r.url);
  if (viaBlob) {
    const blob = await (await fetch(r.url)).blob();
    v.src = URL.createObjectURL(blob);
  } else {
    v.src = r.url;
  }
  const t0 = performance.now();
  r.load = await once(v, ["loadedmetadata", "error"], 8000);
  r.load_ms = Math.round(performance.now() - t0);
  if (r.load !== "loadedmetadata") {
    r.error = v.error ? { code: v.error.code, message: v.error.message } : null;
    return r;
  }
  r.duration = v.duration;
  r.size = [v.videoWidth, v.videoHeight];
  const target = Math.min(30, v.duration / 2);
  const t1 = performance.now();
  v.currentTime = target;
  r.seek = await once(v, ["seeked", "error"], 8000);
  r.seek_ms = Math.round(performance.now() - t1);
  r.seek_target = target;
  r.current_time = v.currentTime;
  r.decoded_frame = v.readyState >= 2;
  return r;
}

(async () => {
  const v = document.getElementById("v");
  const report = {
    userAgent: navigator.userAgent,
    canPlayType: {
      "mp4 h264/aac": v.canPlayType('video/mp4; codecs="avc1.42E01E, mp4a.40.2"'),
      "mp4 hevc": v.canPlayType('video/mp4; codecs="hvc1.1.6.L93.B0"'),
      "webm vp8/vorbis": v.canPlayType('video/webm; codecs="vp8, vorbis"'),
      "webm vp9/opus": v.canPlayType('video/webm; codecs="vp9, opus"'),
      "mkv": v.canPlayType("video/x-matroska"),
    },
    videos: [],
  };
  try {
    const paths = await invoke("spike_videos");
    for (const p of paths) {
      log("probing " + p);
      report.videos.push(await probe(v, p, false));
    }
    if (paths.length) report.videos.push(await probe(v, paths[0], true));
    const httpBase = await invoke("spike_http_base");
    if (httpBase && paths.length) {
      const name = paths[0].split("/").pop();
      report.videos.push(await probe(v, paths[0], false, httpBase + encodeURIComponent(name)));
    }
  } catch (e) {
    report.fatal = String(e);
  }
  await invoke("spike_report", { report });
})();
