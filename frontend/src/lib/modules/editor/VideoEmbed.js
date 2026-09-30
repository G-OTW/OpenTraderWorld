// Video embed node for TipTap: a link and a thumbnail, never a player.
//
// The node stores four short strings and no bytes: the provider, the video id, the
// canonical watch URL and the poster (an ordinary upload served from /api/files/{id},
// inlined as a data: URI when the doc is submitted). It renders as a *facade*: the poster
// wrapped in an anchor, so opening a doc contacts nobody, the block still reads offline,
// and neither the app nor the website has to allow a third-party iframe.
//
// `renderHTML` below is the wire format. The website's server-side renderer
// (OTWWebsite/lib/render-doc.js) emits exactly this markup from the same attributes, and
// both sanitizer allowlists are sized to it. Change one, change all three.
import { Node, mergeAttributes } from '@tiptap/core';

/** Marks the facade for styling. The only class either sanitizer lets through on a figure. */
export const VIDEO_FIGURE_CLASS = 'otw-video';

// Mirrors `parse_video_url` in core/otw-core/src/video_embed.rs. Used when HTML is pasted
// back in; the insert path gets these values from the server, which is the authority.
const YOUTUBE_HOSTS = ['youtube.com', 'm.youtube.com', 'music.youtube.com', 'youtube-nocookie.com'];

export function parseVideoUrl(raw) {
  let url;
  try {
    url = new URL(String(raw ?? '').trim());
  } catch {
    return null;
  }
  if (url.protocol !== 'https:' && url.protocol !== 'http:') return null;
  const host = url.hostname.replace(/^www\./, '').toLowerCase();
  const parts = url.pathname.split('/').filter(Boolean);

  let id = null;
  if (host === 'youtu.be') id = parts[0] ?? null;
  else if (YOUTUBE_HOSTS.includes(host)) {
    if (parts.length === 1 && parts[0] === 'watch') id = url.searchParams.get('v');
    else if (['embed', 'shorts', 'live', 'v'].includes(parts[0])) id = parts[1] ?? null;
  }
  if (id) return /^[A-Za-z0-9_-]{11}$/.test(id) ? { provider: 'youtube', videoId: id } : null;

  if (host === 'vimeo.com' || host === 'player.vimeo.com') {
    const last = parts[parts.length - 1] ?? '';
    return /^\d{6,12}$/.test(last) ? { provider: 'vimeo', videoId: last } : null;
  }
  return null;
}

/** The canonical page, rebuilt from provider + id rather than carried over from input. */
export function watchUrl(provider, videoId) {
  if (provider === 'youtube') return `https://www.youtube.com/watch?v=${videoId}`;
  if (provider === 'vimeo') return `https://vimeo.com/${videoId}`;
  return null;
}

export const VideoEmbed = Node.create({
  name: 'videoEmbed',
  group: 'block',
  atom: true,
  draggable: true,

  addAttributes() {
    return {
      provider: { default: null },
      videoId: { default: null },
      url: { default: null },
      title: { default: null },
      poster: { default: null }
    };
  },

  parseHTML() {
    return [
      {
        tag: `figure.${VIDEO_FIGURE_CLASS}`,
        getAttrs: (el) => {
          const href = el.querySelector('a[href]')?.getAttribute('href') ?? '';
          const ref = parseVideoUrl(href);
          if (!ref) return false;
          return {
            ...ref,
            url: watchUrl(ref.provider, ref.videoId),
            title: el.querySelector('figcaption')?.textContent?.trim() || null,
            poster: el.querySelector('img')?.getAttribute('src') ?? null
          };
        }
      }
    ];
  },

  renderHTML({ node }) {
    const { provider, videoId, title, poster } = node.attrs;
    // The href is rebuilt here too, so a hand-edited `url` attribute cannot become the
    // link a reader clicks.
    const href = watchUrl(provider, videoId) ?? node.attrs.url;
    const label = title || href || '';
    const inner = poster ? ['img', { src: poster, alt: label }] : label;
    return [
      'figure',
      { class: VIDEO_FIGURE_CLASS },
      [
        'a',
        mergeAttributes({ href, target: '_blank', rel: 'noopener noreferrer nofollow' }),
        inner
      ],
      ['figcaption', {}, label]
    ];
  },

  addCommands() {
    return {
      // `attrs` is the resolver's answer: {provider, video_id, url, title, poster}.
      setVideoEmbed:
        (attrs) =>
        ({ commands }) =>
          commands.insertContent({
            type: this.name,
            attrs: {
              provider: attrs.provider ?? null,
              videoId: attrs.video_id ?? attrs.videoId ?? null,
              url: attrs.url ?? null,
              title: attrs.title ?? null,
              poster: attrs.poster ?? null
            }
          })
    };
  },

  // In the editor the facade is inert: a click selects the block instead of navigating,
  // so an author cannot lose their place by clipping the thumbnail.
  addNodeView() {
    return ({ node, editor, getPos }) => {
      const dom = document.createElement('figure');
      dom.className = VIDEO_FIGURE_CLASS;

      const { provider, videoId, title, poster } = node.attrs;
      const href = watchUrl(provider, videoId) ?? node.attrs.url ?? '';
      const label = title || href;

      const a = document.createElement('a');
      a.href = href;
      a.target = '_blank';
      a.rel = 'noopener noreferrer nofollow';
      a.addEventListener('click', (e) => {
        e.preventDefault();
        if (typeof getPos === 'function') editor.commands.setNodeSelection(getPos());
      });
      if (poster) {
        const img = document.createElement('img');
        img.src = poster;
        img.alt = label;
        a.appendChild(img);
      } else {
        a.textContent = label;
      }
      dom.appendChild(a);

      const cap = document.createElement('figcaption');
      cap.textContent = label;
      dom.appendChild(cap);

      return { dom };
    };
  }
});
