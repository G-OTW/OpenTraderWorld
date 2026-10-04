<script>
  import LegalDocument from './LegalDocument.svelte';

  const sections = [
    { id: 'scope', paragraphs: 2, bullets: 3 },
    { id: 'advice', paragraphs: 1, bullets: 4 },
    { id: 'testing', paragraphs: 1, bullets: 4 },
    { id: 'responsibility', paragraphs: 1, bullets: 4 },
    { id: 'integrations', paragraphs: 2, bullets: 3 },
    { id: 'availability', paragraphs: 1, bullets: 3 },
    { id: 'warranty', paragraphs: 2 },
    { id: 'liability', paragraphs: 2 },
    { id: 'indemnity', paragraphs: 1 },
    { id: 'general', paragraphs: 3 }
  ];
</script>

<LegalDocument prefix="settings.terms" icon="file-text" {sections} showFooter={false} />
