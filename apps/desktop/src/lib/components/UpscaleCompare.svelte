<script lang="ts">
  // Before/after still in the preset's shape: the processed picture over a plain bicubic one,
  // revealed by a slider (drag, or arrow keys when focused).
  import { upscale } from "../stores/upscale.svelte";

  let split = $state(50);
  const p = $derived(upscale.preview);
</script>

<figure class="compare" aria-label="Before and after">
  {#if p}
    <img class="before" src={p.before} alt="Before: plain scaling" draggable="false" />
    <img class="after" src={p.after} alt="After: rmedia upscale" draggable="false" style="clip-path: inset(0 0 0 {split}%)" />
    <div class="divider" style="left: {split}%" aria-hidden="true"><span>⇆</span></div>
    <span class="tag left">Before</span>
    <span class="tag right">After</span>
    <input class="slider" type="range" min="0" max="100" step="1" bind:value={split} aria-label="Before/after split" />
  {:else if !upscale.previewError}
    <div class="empty"><div class="spinner" aria-hidden="true"></div></div>
  {/if}
  {#if upscale.previewBusy && p}<div class="busy" aria-hidden="true"><div class="spinner"></div></div>{/if}
  {#if upscale.previewError}
    <p class="err" role="alert">{upscale.previewError}</p>
  {:else if p?.approximate}
    <span class="badge" title="The crop follows the subject once the analysis is done">Centred until analysed</span>
  {:else if p?.in_fallback}
    <span class="badge" title="No single subject fits here, so the whole frame is shown over a blurred copy">Blurred background here</span>
  {/if}
</figure>

<style>
  .compare { position: relative; margin: 0; width: 100%; height: 100%; border-radius: var(--radius-l); overflow: hidden; background: var(--stage-bg); box-shadow: 0 0 0 1px color-mix(in srgb, var(--border) 60%, transparent), var(--shadow); user-select: none; }
  img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; pointer-events: none; }
  .divider { position: absolute; top: 0; bottom: 0; width: 2px; transform: translateX(-1px); background: rgba(255, 255, 255, 0.9); box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.3); pointer-events: none; }
  .divider span { position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%); display: grid; place-items: center; width: 26px; height: 26px; border-radius: 50%; font-size: 12px; color: #171a24; background: #fff; box-shadow: var(--shadow); }
  .slider { position: absolute; inset: 0; width: 100%; height: 100%; margin: 0; opacity: 0; cursor: ew-resize; }
  .slider:focus-visible + * , .compare:focus-within .divider span { outline: 2px solid var(--accent); outline-offset: 2px; }
  .tag { position: absolute; top: var(--space-2); padding: 1px 7px; border-radius: 999px; font-size: 10.5px; font-weight: 650; letter-spacing: 0.02em; color: #fff; background: rgba(7, 8, 12, 0.55); pointer-events: none; }
  .tag.left { left: var(--space-2); }
  .tag.right { right: var(--space-2); }
  .badge { position: absolute; left: 50%; bottom: var(--space-2); transform: translateX(-50%); padding: 2px 8px; border-radius: 999px; font-size: 10.5px; white-space: nowrap; color: #fff; background: rgba(7, 8, 12, 0.65); pointer-events: none; }
  .empty, .busy { position: absolute; inset: 0; display: grid; place-items: center; }
  .busy { background: rgba(7, 8, 12, 0.25); pointer-events: none; }
  .err { position: absolute; inset: auto var(--space-2) var(--space-2); margin: 0; font-size: 11.5px; color: var(--danger); background: rgba(7, 8, 12, 0.8); padding: var(--space-1) var(--space-2); border-radius: var(--radius-s); user-select: text; }
</style>
