<script lang="ts">
  import { api, errorText, type PreviewInfo } from "../api";
  import { app, toast } from "../stores/app.svelte";
  import { addClip } from "../stores/queue";
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
      api.thumbnails(p, 12).then((t) => token === loadToken && (thumbs = t)).catch(() => {});
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

  function timelineClick(e: MouseEvent) {
    const el = e.currentTarget as HTMLElement;
    const r = el.getBoundingClientRect();
    seek(((e.clientX - r.left) / r.width) * duration);
  }

  const pct = (t: number | null) => (t == null || !duration ? 0 : (t / duration) * 100);
</script>

<svelte:window onkeydown={onKey} />

<section class="player">
  {#if mode === "empty"}
    <div class="placeholder">
      <div class="logo" aria-hidden="true">▶</div>
      <h2>Pick a source to start clipping</h2>
      <p class="muted">Mark <kbd>I</kbd> in and <kbd>O</kbd> out, then <kbd>Enter</kbd> to add the clip.</p>
    </div>
  {:else}
    <header>
      <h2 title={path}>{path ? baseName(path) : ""}</h2>
      <span class="pill">
        {#if mode === "direct"}Original{:else if mode === "proxy-h264" || mode === "proxy-vp8"}Preview copy{:else if mode === "still"}Still frames{:else if mode === "loading"}Loading…{:else}Unavailable{/if}
      </span>
    </header>

    <div class="stage">
      {#if (mode === "proxy-h264" || mode === "proxy-vp8") && !src}
        <div class="overlay">
          <p>Preparing a preview copy for this format…</p>
          <div class="bar" style="width: 220px"><div style="width: {Math.round(proxyProgress * 100)}%"></div></div>
          <button class="ghost" onclick={() => fallback()}>Use still frames instead</button>
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
        <div class="overlay"><p class="err">{message}</p></div>
      {/if}
    </div>

    <div class="transport">
      <button class="icon" onclick={togglePlay} disabled={mode === "still" || !src} aria-label={playing ? "Pause" : "Play"}>
        {playing ? "❚❚" : "▶"}
      </button>
      <button class="icon ghost" onclick={() => seek(current - frame)} aria-label="Previous frame">‹</button>
      <button class="icon ghost" onclick={() => seek(current + frame)} aria-label="Next frame">›</button>
      <span class="mono time">{fmt(current)} <span class="faint">/ {fmt(duration)}</span></span>
      <span class="spacer"></span>
      <button onclick={() => (inPt = current)} title="Set in point (I)">In <kbd>I</kbd></button>
      <button onclick={() => (inPt == null || current > inPt) && (outPt = current)} title="Set out point (O)">Out <kbd>O</kbd></button>
    </div>

    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="timeline" role="slider" tabindex="-1" aria-label="Timeline" aria-valuemin={0} aria-valuemax={duration} aria-valuenow={current} onclick={timelineClick}>
      <div class="thumbs">
        {#each thumbs as t, i (i)}<img src={t} alt="" />{/each}
      </div>
      {#if inPt != null}
        <div class="range" style="left: {pct(inPt)}%; width: {pct(outPt ?? duration) - pct(inPt)}%"></div>
      {/if}
      <div class="playhead" style="left: {pct(current)}%"></div>
    </div>

    <div class="clipbar">
      <span class="mono">In {fmt(inPt)}</span>
      <span class="mono">Out {outPt == null ? "end" : fmt(outPt)}</span>
      <input type="text" placeholder="Clip name (optional)" bind:value={clipName} onkeydown={(e) => e.key === "Enter" && addCurrentClip()} />
      <button class="primary" disabled={!canAdd} onclick={addCurrentClip}>Add clip <kbd>↵</kbd></button>
    </div>
    {#if message && mode === "still"}<p class="faint note">Playback isn't available for this file here; showing exact still frames. {message}</p>{/if}
  {/if}
</section>

<style>
  .player { display: flex; flex-direction: column; gap: var(--space-3); padding: var(--space-4); min-height: 0; height: 100%; }
  header { display: flex; align-items: center; gap: var(--space-3); }
  h2 { margin: 0; font-size: 16px; font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .stage { position: relative; flex: 1; min-height: 200px; border-radius: var(--radius-l); background: #07080c; overflow: hidden; display: grid; place-items: center; }
  .stage video, .stage img { width: 100%; height: 100%; object-fit: contain; }
  .overlay { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: var(--space-3); color: #d9dbe5; background: rgba(7, 8, 12, 0.7); }
  .overlay .ghost { color: #d9dbe5; }
  .err { color: #ff8f8f; max-width: 80%; text-align: center; }
  .transport, .clipbar { display: flex; align-items: center; gap: var(--space-2); }
  .time { margin-left: var(--space-2); font-size: 13px; }
  .spacer { flex: 1; }
  .timeline { position: relative; height: 54px; border-radius: var(--radius-m); background: var(--surface-2); overflow: hidden; cursor: pointer; }
  .thumbs { position: absolute; inset: 0; display: flex; opacity: 0.85; }
  .thumbs img { flex: 1; min-width: 0; height: 100%; object-fit: cover; }
  .range { position: absolute; top: 0; bottom: 0; background: color-mix(in srgb, var(--accent) 35%, transparent); border-left: 2px solid var(--accent); border-right: 2px solid var(--accent); }
  .playhead { position: absolute; top: -2px; bottom: -2px; width: 2px; background: #fff; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.4); transform: translateX(-1px); }
  .clipbar input { flex: 1 1 80px; min-width: 0; }
  .clipbar .mono { font-size: 12.5px; color: var(--text-2); white-space: nowrap; }
  .placeholder { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; gap: var(--space-2); }
  .placeholder h2 { font-size: 18px; }
  .logo { width: 56px; height: 56px; border-radius: 16px; display: grid; place-items: center; font-size: 22px; color: white; background: linear-gradient(135deg, var(--accent-2), var(--accent)); box-shadow: var(--shadow); }
  .note { margin: 0; font-size: 12px; }
</style>
