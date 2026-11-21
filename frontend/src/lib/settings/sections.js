// Settings rail sections, shared by the settings page and the global search.
// `labelKey` resolves through $t at render time so the rail relabels on language change.
// The rail is grouped: each group is a titled block, and `sections` stays the flat list
// the hash lookup and the content switch read from.
export const SETTINGS_GROUPS = [
  {
    labelKey: 'settings.group.general',
    items: [
      { id: 'account', labelKey: 'settings.nav.account', icon: 'user' },
      { id: 'appearance', labelKey: 'settings.nav.appearance', icon: 'droplet' },
      { id: 'defaults', labelKey: 'settings.nav.defaults', icon: 'settings' },
      { id: 'modules', labelKey: 'settings.nav.modules', icon: 'grid' }
    ]
  },
  {
    labelKey: 'settings.group.access',
    items: [
      { id: 'security', labelKey: 'settings.nav.security', icon: 'shield' },
      { id: 'network', labelKey: 'settings.nav.network', icon: 'globe' },
      { id: 'vault', labelKey: 'settings.nav.vault', icon: 'lock' }
    ]
  },
  {
    labelKey: 'settings.group.connections',
    items: [
      { id: 'connectors', labelKey: 'settings.nav.connectors', icon: 'plug' },
      { id: 'brokers', labelKey: 'settings.nav.brokers', icon: 'briefcase' },
      { id: 'sessions', labelKey: 'settings.nav.sessions', icon: 'clock' },
      { id: 'exchanges', labelKey: 'settings.nav.exchanges', icon: 'landmark' },
      { id: 'notifications', labelKey: 'settings.nav.notifications', icon: 'bell' },
      { id: 'mcp', labelKey: 'settings.nav.mcp', icon: 'zap' },
      { id: 'control', labelKey: 'settings.nav.control', icon: 'message-square' }
    ]
  },
  {
    labelKey: 'settings.group.voice',
    items: [
      { id: 'voice', labelKey: 'settings.nav.voice', icon: 'mic' },
      { id: 'voice-commands', labelKey: 'settings.nav.voiceCommands', icon: 'audio-lines' }
    ]
  },
  {
    labelKey: 'settings.group.data',
    items: [
      { id: 'data', labelKey: 'settings.nav.data', icon: 'database' },
      { id: 'chart-cache', labelKey: 'settings.nav.chartCache', icon: 'candlestick' },
      { id: 'versioning', labelKey: 'settings.nav.versioning', icon: 'history' },
      { id: 'backup', labelKey: 'settings.nav.backup', icon: 'save' }
    ]
  },
  {
    labelKey: 'settings.group.system',
    items: [
      { id: 'update', labelKey: 'settings.nav.update', icon: 'refresh-cw' },
      { id: 'logs', labelKey: 'settings.nav.logs', icon: 'file-text' },
      { id: 'rate', labelKey: 'settings.nav.rate', icon: 'bar-chart' },
      { id: 'credits', labelKey: 'settings.nav.credits', icon: 'book-open' },
      { id: 'about', labelKey: 'settings.nav.about', icon: 'info' }
    ]
  },
  {
    labelKey: 'settings.group.legal',
    items: [
      { id: 'terms', labelKey: 'settings.nav.terms', icon: 'file-text' },
      { id: 'license', labelKey: 'settings.nav.license', icon: 'book-open' },
      { id: 'risks', labelKey: 'settings.nav.risks', icon: 'alert-triangle' }
    ]
  }
];

export const SETTINGS_SECTIONS = SETTINGS_GROUPS.flatMap((g) => g.items);
