// Resizable image node for TipTap. Extends the base Image extension with a
// `width` attribute and a node-view that shows a selection border plus a
// bottom-right drag handle for resizing. No external dependency.
import Image from '@tiptap/extension-image';

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
        const width = Math.round(parseFloat(img.style.width));
        if (typeof getPos === 'function') {
          editor
            .chain()
            .focus()
            .command(({ tr }) => {
              tr.setNodeMarkup(getPos(), undefined, { ...node.attrs, width });
              return true;
            })
            .run();
        }
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
          return true;
        }
      };
    };
  }
});
