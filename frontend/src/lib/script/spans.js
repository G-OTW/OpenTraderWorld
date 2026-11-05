/** Engine spans count lines and columns in code points (1-based); CodeMirror offsets count
 *  UTF-16 units over the whole text. An emoji before an error is one column for the engine
 *  and two units for the editor, so the squiggle needs this conversion. */
export function spanToRange(text, span) {
  const lines = text.split('\n');
  let from = 0;
  const lineNo = Math.min(Math.max(span.line, 1), lines.length);
  for (let k = 0; k < lineNo - 1; k++) from += lines[k].length + 1;
  const line = lines[lineNo - 1] ?? '';
  // Code point column → UTF-16 offset within the line.
  const unitAt = (col) => {
    let units = 0;
    let cp = 1;
    for (const ch of line) {
      if (cp >= col) break;
      units += ch.length;
      cp++;
    }
    return units;
  };
  const start = span.line > lines.length ? line.length : unitAt(Math.max(span.col, 1));
  const end = Math.min(unitAt(Math.max(span.col, 1) + Math.max(span.len, 0)), line.length);
  return { from: Math.min(from + start, text.length), to: Math.min(from + Math.max(start, end), text.length) };
}
