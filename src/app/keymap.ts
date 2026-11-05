/**
 * Keybindings: canonical chord strings and the global dispatcher.
 * Editor keymaps run first; a key the editor handled (defaultPrevented) is
 * never also dispatched as a command.
 */
import type { CommandContribution } from "@margin/sdk";

const MOD_ORDER = ["ctrl", "alt", "shift", "meta"] as const;

/** `Ctrl+Shift+K` → `ctrl+shift+k`; modifier order is normalized. */
export function normalizeChord(chord: string): string {
  const parts = chord
    .split("+")
    .map((p) => p.trim().toLowerCase())
    .filter(Boolean);
  const mods = MOD_ORDER.filter((m) => parts.includes(m) || (m === "ctrl" && parts.includes("control")));
  const key = parts.filter((p) => !(MOD_ORDER as readonly string[]).includes(p) && p !== "control").at(-1) ?? "";
  return [...mods, key].join("+");
}

const KEY_ALIASES: Record<string, string> = {
  " ": "space",
  arrowleft: "left",
  arrowright: "right",
  arrowup: "up",
  arrowdown: "down",
  escape: "esc",
};

/** Canonical chord for a keyboard event, layout-independent for letters and digits. */
export function chordOf(e: Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): string {
  let key: string;
  if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3).toLowerCase();
  else if (/^Digit[0-9]$/.test(e.code)) key = e.code.slice(5);
  else key = e.key.toLowerCase();
  key = KEY_ALIASES[key] ?? key;
  const mods = [e.ctrlKey && "ctrl", e.altKey && "alt", e.shiftKey && "shift", e.metaKey && "meta"].filter(Boolean);
  return [...mods, key].join("+");
}

export function prettyChord(chord: string): string {
  return chord
    .split("+")
    .map((p) => (p.length === 1 ? p.toUpperCase() : p[0]!.toUpperCase() + p.slice(1)))
    .join("+");
}

export function buildKeymap(
  commands: Iterable<CommandContribution>,
  overrides: Record<string, string | null> = {},
): Map<string, CommandContribution[]> {
  const map = new Map<string, CommandContribution[]>();
  for (const cmd of commands) {
    const binding = cmd.id in overrides ? overrides[cmd.id] : cmd.keybinding;
    if (!binding) continue;
    const chord = normalizeChord(binding);
    map.set(chord, [...(map.get(chord) ?? []), cmd]);
  }
  return map;
}

/** Find the command to run for a key event, if any. */
export function resolve(map: Map<string, CommandContribution[]>, e: KeyboardEvent): CommandContribution | undefined {
  if (e.defaultPrevented || e.isComposing) return undefined;
  const cmds = map.get(chordOf(e));
  return cmds?.find((c) => !c.when || c.when());
}
