// Collapse a multi-block selection into ONE code block.
//
// TipTap's built-in toggleCodeBlock runs ProseMirror's setBlockType, which
// converts each selected textblock on its own and never joins adjacent code
// blocks — selecting three paragraphs yields three code blocks. This replaces
// the whole range with a single code_block whose text is those blocks joined
// by newlines. Collapsed / single-block selections fall through to the default.

/** @param {import('@tiptap/core').Editor} editor */
export function toggleCodeBlockMerged(editor) {
  const { state } = editor;
  const { $from, $to, empty } = state.selection;
  const type = state.schema.nodes.codeBlock;

  // Nothing to merge: let the standard command handle it (including untoggle).
  if (!type || empty || editor.isActive('codeBlock')) {
    return editor.chain().focus().toggleCodeBlock().run();
  }

  // Widen to whole blocks so half-selected paragraphs are taken entirely.
  const from = $from.start($from.depth);
  const to = $to.end($to.depth);
  if (from >= to) return editor.chain().focus().toggleCodeBlock().run();

  const lines = [];
  state.doc.nodesBetween(from, to, (node) => {
    if (!node.isTextblock) return true;
    lines.push(node.textContent);
    return false; // don't descend into inline content
  });
  if (lines.length < 2) return editor.chain().focus().toggleCodeBlock().run();

  const text = lines.join('\n');
  return editor
    .chain()
    .focus()
    .command(({ tr, dispatch }) => {
      if (!dispatch) return true;
      tr.replaceRangeWith(from, to, type.create(null, text ? state.schema.text(text) : null));
      return true;
    })
    .run();
}
