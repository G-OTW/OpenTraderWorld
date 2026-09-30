// Resizable image node for TipTap. Extends the base Image extension with a
// `width` attribute and a node-view that shows a selection border, a
// bottom-right drag handle, and small/medium/large quick-resize buttons.
// No external dependency.
import Image from '@tiptap/extension-image';

// Same glyph as Icon.svelte's "image" icon, rendered at increasing sizes so
// the three quick-resize buttons read as small/medium/large image icons.
const IMAGE_GLYPH =
  '<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.09-3.09a2 2 0 0 0-2.82 0L6 21"/>';

const SIZE_PRESETS = [
  { label: 'Small', fraction: 0.25, icon: 12 },
  { label: 'Medium', fraction: 0.5, icon: 16 },
  { label: 'Large', fraction: 1, icon: 20 }
];

// Same glyph as Icon.svelte's "message-square" icon — used for the note toggle.
const NOTE_GLYPH =
  '<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>';

export const ResizableImage = Image.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      // Stored as a number of pixels; serialized to the style width.
      width: {
        default: null,
        parseHTML: (el) => {
          const w = el.style.width || el.getAttribute('width');
          return w ? parseInt(w, 10) || null : null;
        },
        renderHTML: (attrs) =>
          attrs.width ? { style: `width: ${attrs.width}px` } : {}
      },
      // A free-form note attached to the image, shown in a post-it panel.
      note: {
        default: null,
        parseHTML: (el) => el.getAttribute('data-note') || null,
        renderHTML: (attrs) => (attrs.note ? { 'data-note': attrs.note } : {})
      }
    };
  },

  addNodeView() {
    return ({ node, editor, getPos }) => {
      const dom = document.createElement('span');
      dom.className = 'otw-img';

      const img = document.createElement('img');
      img.src = node.attrs.src;
      if (node.attrs.alt) img.alt = node.attrs.alt;
      if (node.attrs.title) img.title = node.attrs.title;
      if (node.attrs.width) img.style.width = `${node.attrs.width}px`;
      dom.appendChild(img);

      const setAttrs = (patch) => {
        if (typeof getPos !== 'function') return;
        editor
          .chain()
          .focus()
          .command(({ tr }) => {
            tr.setNodeMarkup(getPos(), undefined, { ...node.attrs, ...patch });
            return true;
          })
          .run();
      };

      // Quick-resize S/M/L buttons (shown via CSS only when selected).
      const applyWidth = (width) => {
        img.style.width = `${width}px`;
        setAttrs({ width });
      };

      const toolbar = document.createElement('span');
      toolbar.className = 'otw-img-toolbar';
      for (const { label, fraction, icon } of SIZE_PRESETS) {
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'otw-img-size';
        btn.title = label;
        btn.innerHTML = `<svg width="${icon}" height="${icon}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${IMAGE_GLYPH}</svg>`;
        btn.addEventListener('mousedown', (e) => {
          // Keep the node selected instead of letting the click blur it.
          e.preventDefault();
          e.stopPropagation();
        });
        btn.addEventListener('click', (e) => {
          e.preventDefault();
          e.stopPropagation();
          const containerWidth = dom.closest('.tiptap')?.clientWidth || dom.parentElement?.clientWidth || img.naturalWidth;
          const natural = img.naturalWidth || containerWidth;
          const width = Math.max(40, Math.round(Math.min(containerWidth, natural) * fraction));
          applyWidth(width);
        });
        toolbar.appendChild(btn);
      }

      // Post-it note: a toggle in the toolbar, a folded tab on the image, and a
      // panel that unfolds under it.
      const noteBtn = document.createElement('button');
      noteBtn.type = 'button';
      noteBtn.className = 'otw-img-size';
      noteBtn.title = 'Note';
      noteBtn.innerHTML = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${NOTE_GLYPH}</svg>`;
      toolbar.appendChild(noteBtn);
      dom.appendChild(toolbar);

      const tab = document.createElement('span');
      tab.className = 'otw-img-note-tab';
      tab.title = 'Note';
      tab.innerHTML = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${NOTE_GLYPH}</svg>`;
      dom.appendChild(tab);

      const panel = document.createElement('span');
      panel.className = 'otw-img-note';
      const noteArea = document.createElement('textarea');
      noteArea.className = 'otw-img-note-text';
      noteArea.rows = 3;
      noteArea.placeholder = 'Note…';
      noteArea.value = node.attrs.note ?? '';
      panel.appendChild(noteArea);
      dom.appendChild(panel);

      const syncNote = () => {
        const has = !!(node.attrs.note && node.attrs.note.trim());
        dom.classList.toggle('has-note', has);
      };
      syncNote();

      const openNote = (open) => {
        dom.classList.toggle('note-open', open);
        if (open) noteArea.focus();
      };

      noteBtn.addEventListener('mousedown', (e) => {
        e.preventDefault();
        e.stopPropagation();
      });
      noteBtn.addEventListener('click', (e) => {
        e.preventDefault();
        e.stopPropagation();
        openNote(!dom.classList.contains('note-open'));
      });

      tab.addEventListener('mousedown', (e) => {
        e.preventDefault();
        e.stopPropagation();
      });
      tab.addEventListener('click', (e) => {
        e.preventDefault();
        e.stopPropagation();
        openNote(!dom.classList.contains('note-open'));
      });

      // Keep ProseMirror out of the textarea's own key/selection handling.
      noteArea.addEventListener('mousedown', (e) => e.stopPropagation());
      noteArea.addEventListener('keydown', (e) => e.stopPropagation());
      noteArea.addEventListener('blur', () => {
        const note = noteArea.value.trim() || null;
        if (note !== (node.attrs.note ?? null)) setAttrs({ note });
        if (!note) openNote(false);
      });

      // Bottom-right resize handle (shown via CSS only when selected).
      const handle = document.createElement('span');
      handle.className = 'otw-img-handle';
      dom.appendChild(handle);

      let startX = 0;
      let startW = 0;

      const onMove = (e) => {
        const dx = e.clientX - startX;
        const next = Math.max(40, startW + dx);
        img.style.width = `${next}px`;
      };

      const onUp = () => {
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
        setAttrs({ width: Math.round(parseFloat(img.style.width)) });
      };

      handle.addEventListener('mousedown', (e) => {
        e.preventDefault();
        e.stopPropagation();
        startX = e.clientX;
        startW = img.getBoundingClientRect().width;
        window.addEventListener('mousemove', onMove);
        window.addEventListener('mouseup', onUp);
      });

      return {
        dom,
        // Reflect selection so CSS can draw the border + handle.
        selectNode() {
          dom.classList.add('selected');
        },
        deselectNode() {
          dom.classList.remove('selected');
        },
        update(updatedNode) {
          if (updatedNode.type.name !== node.type.name) return false;
          node = updatedNode;
          img.src = node.attrs.src;
          img.style.width = node.attrs.width ? `${node.attrs.width}px` : '';
          if (document.activeElement !== noteArea) noteArea.value = node.attrs.note ?? '';
          syncNote();
          return true;
        },
        // The textarea lives inside the node view; PM must not own its events.
        stopEvent: (e) => e.target === noteArea || noteArea.contains(e.target),
        ignoreMutation: () => true
      };
    };
  }
});
