<script>
  import LegalDocument from './LegalDocument.svelte';

  const sections = [
    { id: 'rights', paragraphs: 2, bullets: 4 },
    { id: 'competition', paragraphs: 2, bullets: 3 },
    { id: 'distribution', paragraphs: 1, bullets: 3 },
    { id: 'mit', paragraphs: 2, bullets: 3 },
    { id: 'patents', paragraphs: 2, bullets: 2 },
    { id: 'warranty', paragraphs: 2 }
  ];
</script>

<LegalDocument prefix="settings.license" icon="book-open" {sections} showFooter={false} />
