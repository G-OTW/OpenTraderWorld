<script>
  // Long-form legal copy deliberately renders as a document, not as a settings card.
  // The Settings content viewport owns scrolling while the navigation rail stays put.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let { prefix, icon, sections = [], showFooter = true } = $props();
</script>

<article class="legal-document">
  <header class="document-head">
    <div class="title-row">
      <span class="title-icon"><Icon name={icon} size={20} /></span>
      <div>
        <p class="eyebrow">{$t(`${prefix}.eyebrow`)}</p>
        <h2>{$t(`${prefix}.title`)}</h2>
      </div>
    </div>
    <p class="updated">{$t(`${prefix}.updated`)}</p>
  </header>

  <p class="intro">{$t(`${prefix}.intro`)}</p>

  <div class="warning" role="note">
    <Icon name="alert-triangle" size={18} />
    <div>
      <strong>{$t(`${prefix}.warning.title`)}</strong>
      <p>{$t(`${prefix}.warning.body`)}</p>
    </div>
  </div>

  <div class="sections">
    {#each sections as section, index (section.id)}
      <section class="clause">
        <h3><span>{String(index + 1).padStart(2, '0')}</span>{$t(`${prefix}.${section.id}.title`)}</h3>
        <div class="clause-body">
          {#each Array.from({ length: section.paragraphs ?? 0 }) as _, i}
            <p>{$t(`${prefix}.${section.id}.p${i + 1}`)}</p>
          {/each}
          {#if section.bullets}
            <ul>
              {#each Array.from({ length: section.bullets }) as _, i}
                <li>{$t(`${prefix}.${section.id}.b${i + 1}`)}</li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>
    {/each}
  </div>

  {#if showFooter}
    <footer>
      <Icon name="info" size={15} />
      <p>{$t(`${prefix}.footer`)}</p>
    </footer>
  {/if}
</article>

<style>
  .legal-document {
    width: min(100%, 1060px);
    margin: 0 auto;
    padding: var(--space-1) 0 calc(var(--space-8) * 2);
    color: var(--text);
  }
  .document-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-8);
    padding-bottom: var(--space-6);
    border-bottom: var(--hairline) solid var(--border);
  }
  .title-row {
    display: flex;
    align-items: flex-start;
    gap: var(--space-4);
  }
  .title-icon {
    display: inline-flex;
    width: 24px;
    justify-content: center;
    margin-top: 4px;
    color: var(--accent);
  }
  .eyebrow,
  .updated {
    margin: 0;
    color: var(--dim);
    font-size: var(--text-xs);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  h2 {
    margin: var(--space-1) 0 0;
    font-size: var(--text-xl);
    font-weight: var(--fw-medium);
    letter-spacing: -0.01em;
    line-height: 1.2;
  }
  .updated {
    flex: none;
    padding-top: var(--space-1);
    text-align: right;
    line-height: 1.45;
  }
  .intro {
    max-width: 86ch;
    margin: var(--space-6) 0;
    color: var(--muted);
    font-size: var(--text-base);
    line-height: 1.72;
  }
  .warning {
    display: flex;
    align-items: flex-start;
    gap: var(--space-4);
    width: min(100%, 86ch);
    padding: var(--space-4) var(--space-5);
    border-left: var(--active-rule) solid var(--amber);
    border-top: var(--hairline) solid var(--border);
    border-bottom: var(--hairline) solid var(--border);
    color: var(--text);
  }
  .warning > :global(svg) {
    flex: none;
    margin-top: 1px;
    color: var(--amber);
  }
  .warning strong {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    line-height: 1.4;
  }
  .warning p {
    margin: var(--space-1) 0 0;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.62;
  }
  .sections {
    margin-top: var(--space-8);
    border-bottom: var(--hairline) solid var(--border);
  }
  .clause {
    display: grid;
    grid-template-columns: minmax(210px, 0.38fr) minmax(0, 1fr);
    align-items: start;
    gap: var(--space-8);
    padding: calc(var(--space-6) + var(--space-1)) 0;
    border-top: var(--hairline) solid var(--border);
  }
  h3 {
    display: flex;
    gap: var(--space-4);
    align-items: flex-start;
    margin: 0;
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
    line-height: 1.45;
  }
  h3 span {
    min-width: 2em;
    padding-top: 3px;
    color: var(--dim);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    font-weight: var(--fw-normal);
    letter-spacing: 0.04em;
  }
  .clause-body {
    min-width: 0;
    max-width: 72ch;
  }
  .clause-body p,
  .clause-body li {
    color: var(--muted);
    font-size: var(--text-base);
    line-height: 1.7;
  }
  .clause-body p {
    margin: 0 0 var(--space-4);
  }
  .clause-body p:last-child {
    margin-bottom: 0;
  }
  ul {
    margin: var(--space-4) 0 0;
    padding-left: 1.15em;
  }
  li + li {
    margin-top: var(--space-3);
  }
  li::marker {
    color: color-mix(in srgb, var(--accent) 65%, var(--dim));
  }
  footer {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    width: min(100%, 86ch);
    margin-top: var(--space-6);
    padding: 0 var(--space-1);
    color: var(--dim);
  }
  footer > :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  footer p {
    margin: 0;
    font-size: var(--text-sm);
    line-height: 1.62;
  }

  @media (max-width: 1180px) {
    .legal-document {
      width: min(100%, 860px);
    }
    .clause {
      display: block;
      padding: var(--space-6) 0;
    }
    h3 {
      margin-bottom: var(--space-4);
    }
    .clause-body {
      max-width: 78ch;
      padding-left: calc(2em + var(--space-4));
    }
  }

  @media (max-width: 720px) {
    .document-head {
      display: block;
    }
    .updated {
      margin-top: var(--space-3);
      text-align: left;
    }
    .warning {
      padding-right: var(--space-3);
    }
    .clause-body {
      padding-left: 0;
    }
  }
</style>
