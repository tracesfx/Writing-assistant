/**
 * Event bus client (SPEC §5.2). One underlying Tauri listener; handlers
 * subscribe by event type: `events.on("doc.saved", (e) => …)`.
 */
import { events as ipcEvents, type DomainEvent, type MarginEvent } from "./bindings";

export type EventType = DomainEvent["type"];
export type EventOf<K extends EventType> = Extract<DomainEvent, { type: K }>;
type Handler = (e: DomainEvent) => void;

const handlers = new Map<EventType, Set<Handler>>();
let listening: Promise<unknown> | null = null;

/** Deliver an event to subscribers. Exposed for tests and for the shell. */
export function dispatch(envelope: MarginEvent): void {
  const set = handlers.get(envelope.event.type);
  if (!set) return;
  for (const h of [...set]) {
    try {
      h(envelope.event);
    } catch (err) {
      console.error(`[margin] event handler for ${envelope.event.type} failed`, err);
    }
  }
}

function ensureListening(): void {
  if (listening) return;
  listening = ipcEvents.marginEvent.listen((e) => dispatch(e.payload)).catch((err: unknown) => {
    // Outside Tauri (e.g. unit tests) there is no IPC; events can still be dispatched manually.
    console.warn("[margin] event bus unavailable", err);
  });
}

export const events = {
  /** Subscribe to one event type. Returns an unsubscribe function. */
  on<K extends EventType>(type: K, handler: (e: EventOf<K>) => void): () => void {
    ensureListening();
    let set = handlers.get(type);
    if (!set) {
      set = new Set();
      handlers.set(type, set);
    }
    const h = handler as Handler;
    set.add(h);
    return () => {
      set.delete(h);
    };
  },
};
