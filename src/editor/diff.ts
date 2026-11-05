/**
 * Word-level diff for comparing unsaved editor text with the file on disk.
 * Common prefix and suffix are trimmed; the middle uses an LCS table, and
 * very large middles fall back to "all removed, all added".
 */
export type DiffOp = { kind: "equal" | "insert" | "delete"; text: string };

const MAX_CELLS = 4_000_000;

function tokens(s: string): string[] {
  return s.split(/(\s+)/u).filter((t) => t !== "");
}

function push(out: DiffOp[], kind: DiffOp["kind"], text: string): void {
  if (!text) return;
  const last = out.at(-1);
  if (last && last.kind === kind) last.text += text;
  else out.push({ kind, text });
}

export function wordDiff(oldText: string, newText: string): DiffOp[] {
  const a = tokens(oldText);
  const b = tokens(newText);
  let pre = 0;
  while (pre < a.length && pre < b.length && a[pre] === b[pre]) pre++;
  let suf = 0;
  while (suf < a.length - pre && suf < b.length - pre && a[a.length - 1 - suf] === b[b.length - 1 - suf]) suf++;
  const out: DiffOp[] = [];
  push(out, "equal", a.slice(0, pre).join(""));
  const am = a.slice(pre, a.length - suf);
  const bm = b.slice(pre, b.length - suf);
  if (am.length * bm.length > MAX_CELLS) {
    push(out, "delete", am.join(""));
    push(out, "insert", bm.join(""));
  } else {
    const n = am.length;
    const m = bm.length;
    // lcs[i][j] = LCS length of am[i..] and bm[j..]
    const lcs: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
    for (let i = n - 1; i >= 0; i--) {
      for (let j = m - 1; j >= 0; j--) {
        lcs[i]![j] = am[i] === bm[j] ? lcs[i + 1]![j + 1]! + 1 : Math.max(lcs[i + 1]![j]!, lcs[i]![j + 1]!);
      }
    }
    let i = 0;
    let j = 0;
    while (i < n && j < m) {
      if (am[i] === bm[j]) {
        push(out, "equal", am[i]!);
        i++;
        j++;
      } else if (lcs[i + 1]![j]! >= lcs[i]![j + 1]!) {
        push(out, "delete", am[i++]!);
      } else {
        push(out, "insert", bm[j++]!);
      }
    }
    while (i < n) push(out, "delete", am[i++]!);
    while (j < m) push(out, "insert", bm[j++]!);
  }
  push(out, "equal", a.slice(a.length - suf).join(""));
  return out;
}
