<script>
  import LegalDocument from './LegalDocument.svelte';

  const sections = [
    { id: 'capital', paragraphs: 1, bullets: 4 },
    { id: 'markets', paragraphs: 1, bullets: 4 },
    { id: 'leverage', paragraphs: 1, bullets: 4 },
    { id: 'data', paragraphs: 1, bullets: 4 },
    { id: 'models', paragraphs: 1, bullets: 5 },
    { id: 'automation', paragraphs: 1, bullets: 5 },
    { id: 'thirdparty', paragraphs: 1, bullets: 5 },
    { id: 'cyber', paragraphs: 1, bullets: 5 },
    { id: 'privacy', paragraphs: 3, bullets: 4 },
    { id: 'demo', paragraphs: 2 },
    { id: 'controls', paragraphs: 1, bullets: 6 }
  ];
</script>

<LegalDocument prefix="settings.risks" icon="alert-triangle" {sections} />
