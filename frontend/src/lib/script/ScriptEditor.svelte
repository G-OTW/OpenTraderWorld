<script>
  // Pine Script v5/v6 compatible editor: CodeMirror 6, highlighting from `pine-lang.js`,
  // diagnostics from the WASM compiler as you type (same engine as the chart and the server),
  // autocomplete and signature hover from the engine's builtin registry. Loaded lazily: the
  // modal imports this file only when its Script tab opens.
  import { onMount, onDestroy } from 'svelte';
  import { EditorState } from '@codemirror/state';
  import { EditorView, keymap, lineNumbers, hoverTooltip, highlightActiveLine } from '@codemirror/view';
  import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
  import { syntaxHighlighting, defaultHighlightStyle, indentUnit } from '@codemirror/language';
  import { autocompletion } from '@codemirror/autocomplete';
  import { linter, lintGutter } from '@codemirror/lint';
  import { pineLanguage } from './pine-lang.js';
  import { spanToRange } from './spans.js';
  import { compileScript, registryDocs } from './client.js';

  let {
    value = $bindable(''),
    readonly = false,
    // Diagnostic → message (i18n lives with the caller).
    describe = (d) => `${d.code} ${d.args.join(' ')}`,
    // Every compile result, so the caller can list errors and read the script's inputs.
    oncompile
  } = $props();

  let host;
  let view = null;
  let docs = [];

  /** `ta.sma(source, length) → float`, with the phase that brings an unsupported one. */
  function signature(d) {
    const params = d.params.map((p) => (p.default != null ? `${p.name} = ${p.default}` : p.name)).join(', ');
    const head = d.kind === 'func' ? `${d.name}(${params}) → ${d.ret}` : `${d.name}: ${d.ret}`;
    return d.supported ? head : `${head} (${d.phase ? `phase ${d.phase}` : 'not planned'})`;
  }

  const wordAt = (text, pos) => {
    let a = pos;
    let b = pos;
    while (a > 0 && /[\w.]/.test(text[a - 1])) a--;
    while (b < text.length && /[\w]/.test(text[b])) b++;
    return { from: a, to: b, word: text.slice(a, b) };
  };

  function complete(ctx) {
    const w = ctx.matchBefore(/[\w.]+/);
    if (!w || (w.from === w.to && !ctx.explicit) || !docs.length) return null;
    return {
      from: w.from,
      options: docs.map((d) => ({
        label: d.name,
        type: d.kind === 'func' ? 'function' : d.kind === 'const' ? 'constant' : 'variable',
        detail: signature(d),
        boost: d.supported ? 1 : -1
      })),
      validFor: /^[\w.]*$/
    };
  }

  const hover = hoverTooltip((v, pos) => {
    const line = v.state.doc.lineAt(pos);
    const { from, to, word } = wordAt(line.text, pos - line.from);
    const d = docs.find((x) => x.name === word);
    if (!d) return null;
    return {
      pos: line.from + from,
      end: line.from + to,
      above: true,
      create() {
        const dom = document.createElement('div');
        dom.className = 'sig';
        dom.textContent = signature(d);
        return { dom };
      }
    };
  });

  const lint = linter(
    async (v) => {
      const src = v.state.doc.toString();
      const r = await compileScript(src);
      oncompile?.(r);
      const list = r.ok ? (r.warnings ?? []) : (r.diagnostics ?? []);
      return list.map((d) => ({
        ...spanToRange(src, d.span),
        severity: d.severity === 'Error' ? 'error' : 'warning',
        message: describe(d)
      }));
    },
    { delay: 300 }
  );

  const theme = EditorView.theme({
    '&': { height: '100%', background: 'var(--surface)', color: 'var(--text)', fontSize: '13px' },
    '.cm-content': { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', caretColor: 'var(--text)' },
    '.cm-gutters': { background: 'var(--surface-2)', color: 'var(--muted)', border: 'none' },
    '.cm-activeLine': { background: 'color-mix(in srgb, var(--accent) 6%, transparent)' },
    '.cm-tooltip': { background: 'var(--surface-2)', border: '1px solid var(--border)', color: 'var(--text)' },
    '.sig': { padding: '2px 6px', fontFamily: 'ui-monospace, monospace', fontSize: '12px' },
    '&.cm-focused': { outline: 'none' }
  });

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          history(),
          highlightActiveLine(),
          indentUnit.of('    '),
          keymap.of([indentWithTab, ...defaultKeymap, ...historyKeymap]),
          pineLanguage,
          syntaxHighlighting(defaultHighlightStyle),
          autocompletion({ override: [complete] }),
          hover,
          lint,
          lintGutter(),
          theme,
          EditorState.readOnly.of(readonly),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) value = u.state.doc.toString();
          })
        ]
      })
    });
    registryDocs()
      .then((d) => (docs = d))
      .catch(() => {
        /* no autocomplete without the registry; editing still works */
      });
  });

  // A value set from outside (another library row picked) replaces the document.
  $effect(() => {
    const v = value;
    if (view && v !== view.state.doc.toString()) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: v } });
    }
  });

  onDestroy(() => view?.destroy());
</script>

<div class="editor" bind:this={host}></div>

<style>
  .editor {
    height: 360px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .editor :global(.cm-editor) {
    height: 100%;
  }
</style>
