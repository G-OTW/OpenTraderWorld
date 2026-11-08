// Code block with a copy button. Extends the base extension with a node view
// that keeps the standard pre > code structure (so editing is untouched) and
// hangs a non-editable button in the corner.
import CodeBlock from '@tiptap/extension-code-block';

const COPY_GLYPH =
  '<rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>';
const CHECK_GLYPH = '<path d="M20 6 9 17l-5-5"/>';

const svg = (glyph) =>
  `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${glyph}</svg>`;

export const CopyableCodeBlock = CodeBlock.extend({
  addNodeView() {
    return ({ node }) => {
      const dom = document.createElement('pre');
      dom.className = 'otw-code';

      const content = document.createElement('code');
      dom.appendChild(content);

      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'otw-code-copy';
      btn.contentEditable = 'false';
      btn.title = 'Copy';
      btn.innerHTML = svg(COPY_GLYPH);
      dom.appendChild(btn);

      let resetTimer;
      btn.addEventListener('mousedown', (e) => {
        e.preventDefault();
        e.stopPropagation();
      });
      btn.addEventListener('click', async (e) => {
        e.preventDefault();
        e.stopPropagation();
        try {
          await navigator.clipboard.writeText(node.textContent);
        } catch {
          return; // clipboard denied (non-secure context) — stay silent
        }
        btn.innerHTML = svg(CHECK_GLYPH);
        btn.classList.add('done');
        clearTimeout(resetTimer);
        resetTimer = setTimeout(() => {
          btn.innerHTML = svg(COPY_GLYPH);
          btn.classList.remove('done');
        }, 1200);
      });

      return {
        dom,
        contentDOM: content,
        update(updated) {
          if (updated.type.name !== node.type.name) return false;
          node = updated;
          return true;
        },
        // The button lives outside contentDOM; PM must ignore it entirely.
        stopEvent: (e) => btn.contains(e.target),
        ignoreMutation: (m) => btn.contains(m.target),
        destroy() {
          clearTimeout(resetTimer);
        }
      };
    };
  }
});
