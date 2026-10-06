<script lang="ts">
  import { api, errorText, type PreviewInfo } from "../api";
  import { app, toast } from "../stores/app.svelte";
  import { addClip } from "../stores/queue";
  import { pickFolder, pickVideos } from "../stores/sources.svelte";
  import { baseName, fmt } from "../time";

  type Mode = "empty" | "loading" | "direct" | "proxy-h264" | "proxy-vp8" | "still" | "error";

  let mode = $state<Mode>("empty");
  let preview = $state<PreviewInfo | null>(null);
  let src = $state<string | null>(null);
  let proxyProgress = $state(0);
  let video = $state<HTMLVideoElement | null>(null);
  let current = $state(0); // clip time (seconds)
  let playing = $state(false);
  let inPt = $state<number | null>(null);
  let outPt = $state<number | null>(null);
  let clipName = $state("");
  let thumbs = $state<string[]>([]);
  let stillUrl = $state<string | null>(null);
  let message = $state("");
  let loadToken = 0;

  const path = $derived(app.selected);
  const duration = $derived(preview?.info.duration ?? 0);
  const offset = $derived(preview?.time_offset ?? 0);
  const frame = $derived.by(() => {
    const v = preview?.info.streams.find((s) => s.kind === "video" && !s.is_attached_pic);
    return v?.frame_rate ? v.frame_rate[1] / v.frame_rate[0] : 1 / 25;
  });
  const canAdd = $derived(!!path && inPt != null && (outPt == null || outPt > inPt));

  $effect(() => {
    const p = path;
    void load(p);
  });

  async function load(p: string | null) {
    const token = ++loadToken;
    preview = null;
    src = null;
    thumbs = [];
    inPt = outPt = null;
    current = 0;
    clipName = "";
    setStill(null);
    if (!p) {
      mode = "empty";
      return;
    }
    mode = "loading";
    try {
      const info = await api.previewSource(p);
      if (token !== loadToken) return;
      preview = info;
      if (info.direct_url) {
        src = info.direct_url;
        mode = "direct";
      } else {
        await useProxy(p, "h264", token);
      }
      const n = Math.min(30, Math.max(6, Math.ceil((timelineEl?.clientWidth ?? 600) / 96)));
      api.thumbnails(p, n).then((t) => token === loadToken && (thumbs = t)).catch(() => {});
    } catch (e) {
      if (token !== loadToken) return;
      mode = "error";
      message = errorText(e);
    }
  }

  async function useProxy(p: string, codec: "h264" | "vp8", token: number) {
    mode = codec === "h264" ? "proxy-h264" : "proxy-vp8";
    src = null;
    proxyProgress = 0;
    try {
      const url = await api.ensureProxy(p, codec, (f) => (proxyProgress = f));
      if (token === loadToken) src = url;
    } catch (e) {
      if (token === loadToken) await fallback(errorText(e));
    }
  }

  /** The webview could not play the current source: try the next transport. */
  async function fallback(reason = "") {
    const p = path;
    if (!p) return;
    const token = loadToken;
    if (mode === "direct") return useProxy(p, "h264", token);
    if (mode === "proxy-h264") return useProxy(p, "vp8", token);
    mode = "still";
    message = reason;
    src = null;
    await showStill(current);
  }

  function setStill(url: string | null) {
    if (stillUrl) URL.revokeObjectURL(stillUrl);
    stillUrl = url;
  }

  let stillTimer: ReturnType<typeof setTimeout> | undefined;
  function showStill(t: number) {
    clearTimeout(stillTimer);
    return new Promise<void>((resolve) => {
      stillTimer = setTimeout(async () => {
        const p = path;
        if (p) {
          try {
            setStill(URL.createObjectURL(await api.frameAt(p, t, 540)));
          } catch (e) {
            message = errorText(e);
          }
        }
        resolve();
      }, 90);
    });
  }

  function seek(t: number) {
    const clamped = Math.min(Math.max(0, t), Math.max(0, duration));
    current = clamped;
    if (mode === "still") void showStill(clamped);
    else if (video) video.currentTime = clamped + offset;
  }

  function onTimeUpdate() {
    if (video) current = Math.max(0, video.currentTime - offset);
  }

  function togglePlay() {
    if (!video || mode === "still") return;
    if (video.paused) void video.play();
    else video.pause();
  }

  function addCurrentClip() {
    if (!canAdd || !path || inPt == null) return;
    const round = (x: number) => Math.round(x * 1000) / 1000;
    app.queue = addClip(app.queue, path, {
      start: round(inPt),
      end: outPt == null ? "end" : round(outPt),
      ...(clipName.trim() ? { name: clipName.trim() } : {}),
    });
    toast(`Added ${fmt(inPt)} → ${outPt == null ? "end" : fmt(outPt)} to the queue`);
    inPt = outPt;
    outPt = null;
    clipName = "";
  }

  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    if (target.closest("input, select, textarea") || !path || mode === "empty") return;
    const step = e.shiftKey ? 1 : frame;
    switch (e.key) {
      case " ":
        togglePlay();
        break;
      case "i":
      case "I":
        inPt = current;
        if (outPt != null && outPt <= current) outPt = null;
        break;
      case "o":
      case "O":
        if (inPt == null || current > inPt) outPt = current;
        break;
      case "Enter":
      case "a":
      case "A":
        addCurrentClip();
        break;
      case "ArrowLeft":
      case ",":
        seek(current - step);
        break;
      case "ArrowRight":
      case ".":
        seek(current + step);
        break;
      case "Home":
        seek(0);
        break;
      case "End":
        seek(duration);
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  // Timeline: pointer drag scrubs; hover shows the time under the pointer.
  let timelineEl = $state<HTMLElement | null>(null);
  let hoverT = $state<number | null>(null);
  let hoverX = $state(0);
  function timeAt(clientX: number): number {
    if (!timelineEl) return 0;
    const r = timelineEl.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - r.left) / r.width)) * duration;
  }
  function onPointerDown(e: PointerEvent) {
    timelineEl?.setPointerCapture(e.pointerId);
    seek(timeAt(e.clientX));
  }
  function onPointerMove(e: PointerEvent) {
    if (!timelineEl) return;
    hoverX = e.clientX - timelineEl.getBoundingClientRect().left;
    hoverT = timeAt(e.clientX);
    if (timelineEl.hasPointerCapture(e.pointerId)) seek(hoverT);
  }

  async function retry() {
    message = "";
    await load(path);
  }

  const pct = (t: number | null) => (t == null || !duration ? 0 : (t / duration) * 100);
</script>

<svelte:window onkeydown={onKey} />

<section class="player" aria-label="Player">
  {#if mode === "empty"}
    <div class="placeholder">
      <div class="logo" aria-hidden="true">
        <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5.5v13l11-6.5z" /></svg>
      </div>
      <h2>Pick a source to start clipping</h2>
      <ol class="steps">
        <li><b>1</b> Add a video</li>
        <li><b>2</b> Mark <kbd>I</kbd> in and <kbd>O</kbd> out</li>
        <li><b>3</b> Press <kbd>↵</kbd> to queue the clip</li>
      </ol>
      <div class="row">
        <button class="primary" onclick={pickVideos}>Add videos</button>
        <button onclick={pickFolder}>Add folder</button>
      </div>
      <p class="faint">…or drop files and folders anywhere in the window.</p>
    </div>
  {:else}
    <header class="pane-head">
      <h2 title={path}>{path ? baseName(path) : ""}</h2>
      {#if mode === "direct"}
        <span class="pill" title="Playing the original file">Original</span>
      {:else if mode === "proxy-h264" || mode === "proxy-vp8"}
        <span class="pill" title="Playing a lightweight copy for this format; clips are always cut from the original">Preview copy</span>
      {:else if mode === "still"}
        <span class="pill" title="This format can't play here; scrub with exact still frames">Still frames</span>
      {/if}
    </header>

    <div class="stage">
      {#if mode === "loading"}
        <div class="overlay"><div class="spinner" aria-hidden="true"></div><p>Opening…</p></div>
      {/if}
      {#if (mode === "proxy-h264" || mode === "proxy-vp8") && !src}
        <div class="overlay">
          <p>Preparing a preview copy for this format… {Math.round(proxyProgress * 100)}%</p>
          <div class="bar" style="width: 220px" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(proxyProgress * 100)}>
            <div style="width: {Math.round(proxyProgress * 100)}%"></div>
          </div>
          <button class="ghost small" onclick={() => fallback()}>Use still frames instead</button>
        </div>
      {/if}
      {#if mode === "still"}
        {#if stillUrl}<img src={stillUrl} alt="Frame at {fmt(current)}" />{/if}
      {:else if src}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video
          bind:this={video}
          {src}
          preload="auto"
          playsinline
          onerror={() => fallback()}
          ontimeupdate={onTimeUpdate}
          onseeked={onTimeUpdate}
          onplay={() => (playing = true)}
          onpause={() => (playing = false)}
        ></video>
      {/if}
      {#if mode === "error"}
        <div class="overlay">
          <p class="title">Can't open this video</p>
          <p class="err">{message}</p>
          <button onclick={retry}>Retry</button>
        </div>
      {/if}
    </div>

    <div class="transport">
      <button class="icon" onclick={togglePlay} disabled={mode === "still" || !src} aria-label={playing ? "Pause (Space)" : "Play (Space)"} title={playing ? "Pause (Space)" : "Play (Space)"}>
        {#if playing}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="5" width="4" height="14" rx="1" /><rect x="14" y="5" width="4" height="14" rx="1" /></svg>
        {:else}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5.5v13l11-6.5z" /></svg>
        {/if}
      </button>
      <button class="icon ghost" onclick={() => seek(current - frame)} aria-label="Previous frame" title="Previous frame (←)">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M15 6l-6 6 6 6" /></svg>
      </button>
      <button class="icon ghost" onclick={() => seek(current + frame)} aria-label="Next frame" title="Next frame (→)">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M9 6l6 6-6 6" /></svg>
      </button>
      <span class="mono time">{fmt(current)} <span class="faint">/ {fmt(duration)}</span></span>
      <span class="spacer"></span>
      <button class="mark" class:set={inPt != null} onclick={() => { inPt = current; if (outPt != null && outPt <= current) outPt = null; }} title="Set in point (I)">
        In <span class="mono">{inPt == null ? "—" : fmt(inPt)}</span>
      </button>
      <button class="mark" class:set={outPt != null} onclick={() => (inPt == null || current > inPt) && (outPt = current)} title="Set out point (O)">
        Out <span class="mono">{outPt == null ? "end" : fmt(outPt)}</span>
      </button>
    </div>

    <div
      class="timeline"
      bind:this={timelineEl}
      role="slider"
      tabindex="0"
      aria-label="Timeline"
      aria-valuemin={0}
      aria-valuemax={duration}
      aria-valuenow={current}
      aria-valuetext={fmt(current)}
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerleave={() => (hoverT = null)}
    >
      <div class="thumbs" class:loading={thumbs.length === 0}>
        {#each thumbs as t, i (i)}<img src={t} alt="" />{/each}
      </div>
      {#if inPt != null}
        <div class="dim" style="left: 0; width: {pct(inPt)}%"></div>
        <div class="dim" style="left: {pct(outPt ?? duration)}%; right: 0"></div>
        <div class="range" class:open={outPt == null} style="left: {pct(inPt)}%; width: {pct(outPt ?? duration) - pct(inPt)}%"></div>
      {/if}
      <div class="playhead" style="left: {pct(current)}%"></div>
      {#if hoverT != null}
        <div class="tip mono" style="left: {hoverX}px">{fmt(hoverT)}</div>
      {/if}
    </div>

    <div class="clipbar">
      <input type="text" placeholder="Clip name (optional)" bind:value={clipName} onkeydown={(e) => e.key === "Enter" && addCurrentClip()} />
      <button class="primary" disabled={!canAdd} onclick={addCurrentClip}>Add clip <kbd>↵</kbd></button>
    </div>
    <p class="hints faint">
      <kbd>Space</kbd> play · <kbd>I</kbd>/<kbd>O</kbd> mark · <kbd>←</kbd>/<kbd>→</kbd> frame · <kbd>⇧</kbd>+arrows 1 s · <kbd>↵</kbd> add
    </p>
    {#if message && mode === "still"}<p class="faint note">Playback isn't available for this file here; showing exact still frames. {message}</p>{/if}
  {/if}
</section>

<style>
  .player { display: flex; flex-direction: column; gap: var(--space-3); padding: var(--space-4); min-height: 0; height: 100%; }
  header { display: flex; align-items: center; gap: var(--space-3); }
  h2 { margin: 0; font-size: 16px; font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .stage { position: relative; flex: 1; min-height: 160px; border-radius: var(--radius-l); background: var(--stage-bg); overflow: hidden; display: grid; place-items: center; }
  .stage video, .stage img { width: 100%; height: 100%; object-fit: contain; }
  .overlay { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: var(--space-3); color: #d9dbe5; background: rgba(7, 8, 12, 0.7); }
  .overlay .ghost { color: #d9dbe5; }
  .err { color: var(--danger); max-width: 80%; text-align: center; margin: 0; user-select: text; }
  .overlay .title { font-weight: 650; margin: 0; }
  .transport, .clipbar { display: flex; align-items: center; gap: var(--space-2); }
  .time { margin-left: var(--space-2); font-size: 13px; }
  .spacer { flex: 1; }
  .timeline { position: relative; height: 54px; border-radius: var(--radius-m); background: var(--surface-2); cursor: pointer; touch-action: none; flex-shrink: 0; }
  .thumbs { position: absolute; inset: 0; display: flex; border-radius: var(--radius-m); overflow: hidden; }
  .thumbs img { flex: 1; min-width: 0; height: 100%; object-fit: cover; animation: fade 200ms var(--ease); }
  .thumbs.loading { background: linear-gradient(90deg, var(--surface-2), var(--surface), var(--surface-2)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @keyframes fade { from { opacity: 0; } }
  .dim { position: absolute; top: 0; bottom: 0; background: rgba(0, 0, 0, 0.45); }
  .range { position: absolute; top: 0; bottom: 0; background: color-mix(in srgb, var(--accent) 22%, transparent); border-left: 3px solid var(--accent); border-right: 3px solid var(--accent); }
  .range.open { border-right-style: dashed; }
  .playhead { position: absolute; top: -3px; bottom: -3px; width: 2px; background: #fff; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.45); transform: translateX(-1px); z-index: 2; pointer-events: none; }
  .playhead::before { content: ""; position: absolute; top: -5px; left: -4px; width: 10px; height: 10px; border-radius: 50%; background: #fff; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.45); }
  .tip { position: absolute; bottom: calc(100% + 6px); transform: translateX(-50%); padding: 2px 6px; border-radius: 5px; font-size: 11px; background: var(--surface); border: 1px solid var(--border); box-shadow: var(--shadow); pointer-events: none; white-space: nowrap; z-index: 3; }
  .clipbar { flex-wrap: wrap; }
  .clipbar input { flex: 1 1 140px; min-width: 0; }
  .mark { gap: 6px; }
  .mark .mono { font-size: 12px; color: var(--text-2); }
  .mark.set { background: var(--accent-soft); border-color: color-mix(in srgb, var(--accent) 50%, transparent); }
  .mark.set .mono { color: var(--text); }
  .hints { margin: 0; font-size: 11.5px; display: flex; flex-wrap: wrap; gap: 4px; align-items: center; }
  .placeholder { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; gap: var(--space-2); }
  .placeholder h2 { font-size: 18px; }
  .placeholder .row { display: flex; gap: var(--space-2); margin-top: var(--space-2); }
  .steps { list-style: none; padding: 0; margin: var(--space-2) 0; display: flex; gap: var(--space-4); color: var(--text-2); }
  .steps b { display: inline-grid; place-items: center; width: 20px; height: 20px; margin-right: 6px; border-radius: 50%; font-size: 11px; background: var(--accent-soft); color: var(--accent); }
  .logo { width: 56px; height: 56px; border-radius: 16px; display: grid; place-items: center; color: var(--accent-fg); background: linear-gradient(135deg, var(--accent-2), var(--accent)); box-shadow: var(--shadow); }
  .note { margin: 0; font-size: 12px; }
</style>
