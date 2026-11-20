<script lang="ts">
  // The clip panel's video target (feature 005): None, a target, or custom limits; the
  // prediction for the marked range; and a refusal with "Allow trimming" when that would fit.
  import { fit } from "../stores/fit.svelte";
  import { fitLine, fitNotes, refusal } from "../stores/fit";

  let { marked }: { marked: boolean } = $props();

  const c = $derived(fit.choice);
  const targets = $derived(fit.targets?.targets ?? []);
  const refused = $derived(refusal(fit.issues));
  const notes = $derived(fitNotes(fit.notices));

  function set(patch: Partial<typeof fit.choice>) {
    fit.choice = { ...fit.choice, ...patch };
  }
  function pick(value: string) {
    if (value === "") set({ target: null, custom: false });
    else if (value === "custom") set({ target: c.target ?? fit.targets?.default ?? "reddit-video", custom: true });
    else set({ target: value, custom: false });
  }
</script>

<div class="fit" aria-label="Video target">
  <div class="row">
    <label for="fit-target" class="section-title">Video target</label>
    <select id="fit-target" value={c.target == null ? "" : c.custom ? "custom" : c.target} onchange={(e) => pick(e.currentTarget.value)}>
      <option value="">None</option>
      {#each targets as t (t.id)}
        <option value={t.id}>{t.name}{t.verified ? "" : " (limits unverified)"}</option>
      {/each}
      <option value="custom">Custom limits…</option>
    </select>
    {#if c.custom}
      <select aria-label="Custom limits start from" value={c.target ?? ""} onchange={(e) => set({ target: e.currentTarget.value })}>
        {#each targets as t (t.id)}<option value={t.id}>{t.name}</option>{/each}
      </select>
      <input class="limit" type="text" placeholder="max size, e.g. 50MB" aria-label="Maximum size" value={c.maxSize} onchange={(e) => set({ maxSize: e.currentTarget.value })} />
      <input class="limit" type="text" placeholder="max length, e.g. 5:00" aria-label="Maximum length" value={c.maxLength} onchange={(e) => set({ maxLength: e.currentTarget.value })} />
    {/if}
    {#if c.target}
      <label class="check" title="Keep the start of a range that is too long, or too big for the cap, instead of refusing it">
        <input type="checkbox" checked={c.allowTrim} onchange={(e) => set({ allowTrim: e.currentTarget.checked })} />
        Allow trimming
      </label>
    {/if}
  </div>
  {#if c.target}
    <p class="line" role="status" class:bad={!!refused || !!fit.planError}>
      {#if !marked}
        <span class="faint">Mark a range to see the predicted size</span>
      {:else if fit.planError}
        {fit.planError}
      {:else if refused}
        <span>{refused.message}</span>
        {#if refused.trimmable && !c.allowTrim}
          <button class="small" onclick={() => set({ allowTrim: true })}>Allow trimming</button>
        {/if}
      {:else if fit.summary}
        <span>{fitLine(fit.summary, targets)}</span>
        {#if !fit.summary.target_verified}<span class="badge" title="Reddit's limits change; check its Help Center">limits unverified</span>{/if}
      {:else}
        <span class="faint">Predicting…</span>
      {/if}
    </p>
    {#each notes as n, i (i)}<p class="faint note">{n}</p>{/each}
  {/if}
</div>

<style>
  .fit { display: flex; flex-direction: column; gap: 4px; }
  .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
  .row .section-title { margin: 0; }
  .limit { width: 150px; min-width: 0; }
  .check { display: flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--text-2); }
  .line { margin: 0; font-size: 12.5px; display: flex; flex-wrap: wrap; align-items: center; gap: 4px 12px; }
  .line.bad { color: var(--warn); }
  .badge { font-size: 11px; padding: 0 6px; border-radius: 999px; background: var(--surface-2); border: 1px solid var(--border); color: var(--text-2); }
  .note { margin: 0; font-size: 12px; }
  button.small { height: 24px; padding: 0 8px; font-size: 12px; }
</style>
