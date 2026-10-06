// Source actions shared by the Sources pane, the empty player and drag-and-drop.
import { api, errorText } from "../api";
import { baseName } from "../time";
import { app, toast, type SourceItem } from "./app.svelte";
import { merge } from "./queue";

export const scan = $state({ current: null as { id: string; text: string } | null });

function upsert(item: SourceItem) {
  const i = app.sources.findIndex((s) => s.path === item.path);
  if (i >= 0) app.sources[i] = { ...app.sources[i], ...item };
  else app.sources.push(item);
  if (!app.selected) app.selected = item.path;
}

export async function addFiles(paths: string[]) {
  for (const path of paths) {
    upsert({ path });
    try {
      upsert({ path, info: await api.probe(path) });
    } catch (e) {
      upsert({ path, error: errorText(e) });
    }
  }
}

export async function scanFolder(root: string) {
  const id = crypto.randomUUID();
  scan.current = { id, text: `Scanning ${baseName(root)}…` };
  try {
    const result = await api.scan(id, [root], {}, (e) => {
      if (e.type === "progress" && scan.current) {
        scan.current.text = `Checked ${e.probed} of ${e.candidates} candidate files`;
      } else if (e.type === "found") {
        upsert({ path: e.video.path, info: e.video });
      }
    });
    const rejected = result.rejected.length ? `, ${result.rejected.length} skipped (not playable video)` : "";
    toast(`Found ${result.videos.length} video(s) in ${baseName(root)}${rejected}`);
  } catch (e) {
    const msg = errorText(e);
    if (msg !== "cancelled") toast(msg, "error");
  } finally {
    scan.current = null;
  }
}

/** Dropped or command-line paths (already registered by Rust): files are probed, folders scanned. */
export async function openPaths(paths: string[]) {
  for (const p of paths) {
    try {
      upsert({ path: p, info: await api.probe(p) });
    } catch {
      await scanFolder(p);
    }
  }
}

export async function pickVideos() {
  try {
    await addFiles(await api.pickFiles("video"));
  } catch (e) {
    toast(errorText(e), "error");
  }
}

export async function pickFolder() {
  try {
    const dir = await api.pickFolder();
    if (dir) await scanFolder(dir);
  } catch (e) {
    toast(errorText(e), "error");
  }
}

export async function openManifest() {
  try {
    const [path] = await api.pickFiles("manifest");
    if (!path) return;
    const m = await api.loadManifest(path);
    app.queue = merge(app.queue, m);
    if (m.defaults && Object.keys(m.defaults).length) app.queue.defaults = { ...app.queue.defaults, ...m.defaults };
    await addFiles(m.videos.map((v) => v.input));
    toast(`Loaded ${m.videos.length} video(s) from ${baseName(path)}`);
  } catch (e) {
    toast(errorText(e), "error");
  }
}

export function removeSourceItem(path: string) {
  app.sources = app.sources.filter((s) => s.path !== path);
  if (app.selected === path) app.selected = app.sources[0]?.path ?? null;
}
