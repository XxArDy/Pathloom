export type DiffLine = { op: ' ' | '+' | '-'; text: string };

const lines = (s: string) => {
  const t = s.replace(/\r\n/g, '\n');
  return t === '' ? [] : (t.endsWith('\n') ? t.slice(0, -1) : t).split('\n');
};

/** Line diff by LCS; CRLF is normalised to LF. shortcut: O(n*m) memory, fine for stage files. */
export function lineDiff(before: string, after: string): DiffLine[] {
  const a = lines(before);
  const b = lines(after);
  const w = b.length + 1;
  const L = new Uint32Array((a.length + 1) * w);
  for (let i = a.length - 1; i >= 0; i--)
    for (let j = b.length - 1; j >= 0; j--)
      L[i * w + j] = a[i] === b[j] ? L[(i + 1) * w + j + 1] + 1 : Math.max(L[(i + 1) * w + j], L[i * w + j + 1]);
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) { out.push({ op: ' ', text: a[i] }); i++; j++; }
    else if (L[(i + 1) * w + j] >= L[i * w + j + 1]) out.push({ op: '-', text: a[i++] });
    else out.push({ op: '+', text: b[j++] });
  }
  while (i < a.length) out.push({ op: '-', text: a[i++] });
  while (j < b.length) out.push({ op: '+', text: b[j++] });
  return out;
}
