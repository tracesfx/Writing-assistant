import { workbench, BIBLE_ENTRY_VIEW, openSource, type Author, type Source, type Status } from "@margin/sdk";

/** Canon is plain; inferred shows a dotted underline (SPEC §9.1). */
export function StatusMark({ status, by }: { status: Status; by?: Author }) {
  return (
    <span
      className={`mem-status mem-${status}`}
      title={status === "canon" ? "Canon: written or confirmed by you" : `Inferred: read from your text by the model${by === "ai" ? "" : ""}`}
    >
      {status === "canon" ? "canon" : "inferred"}
    </span>
  );
}

/** A link to the passage a record came from. */
export function SourceLink({ source, title }: { source: Source; title?: string }) {
  const q = source.quote.length > 90 ? `${source.quote.slice(0, 90).trimEnd()}…` : source.quote;
  return (
    <button
      type="button"
      className="link mem-source"
      title="Show this passage in the manuscript"
      onClick={() => void openSource({ doc: source.doc, para: source.para ?? null, block: source.block ?? null, quote: source.quote })}
    >
      {title ? `${title}: ` : ""}“{q}”
    </button>
  );
}

export function EntryLink({ id, name }: { id: string; name: string }) {
  return (
    <button type="button" className="link" onClick={() => workbench.openView(BIBLE_ENTRY_VIEW, id)}>
      {name}
    </button>
  );
}

export function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function fail(what: string, e: unknown): void {
  workbench.notify({ message: `${what}: ${errText(e)}` });
}
