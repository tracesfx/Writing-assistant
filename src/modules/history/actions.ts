import { editorHost, story, workbench } from "@margin/sdk";

export const HISTORY_VIEW = "history.document";

export const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));

export async function takeSnapshot(): Promise<void> {
  const name = await workbench.ask({ title: "Name this snapshot", placeholder: "e.g. Before the big rewrite" });
  if (name === null) return;
  try {
    await editorHost.flushAll();
    const snap = await story.history.snapshot(name.trim() || "Snapshot");
    workbench.notify({ message: snap ? `Snapshot “${snap.name}” saved` : "Snapshot saved" });
  } catch (e) {
    workbench.notify({ message: `Could not take a snapshot: ${errorMessage(e)}` });
  }
}
