import { defineConfig } from 'vitepress';

export default defineConfig({
  title: 'OpenTraderWorld',
  description:
    'Self-hosted, modular platform for traders and investors — journal, market data, backtesting, portfolios and more, on your own machine.',
  base: '/OpenTraderWorld/',
  cleanUrls: true,
  lastUpdated: true,
  srcExclude: ['README.md'],

  themeConfig: {
    nav: [
      { text: 'Guide', link: '/guide/introduction', activeMatch: '/guide/' },
      { text: 'Configuration', link: '/config/network', activeMatch: '/config/' },
      { text: 'Modules', link: '/modules/', activeMatch: '/modules/' }
    ],

    sidebar: [
      {
        text: 'Getting started',
        items: [
          { text: 'What is OpenTraderWorld?', link: '/guide/introduction' },
          { text: 'Get Docker', link: '/guide/docker' },
          { text: 'Installation', link: '/guide/install' },
          { text: 'First steps', link: '/guide/first-steps' },
          { text: 'Updating', link: '/guide/updating' },
          { text: 'Backup & restore', link: '/guide/backup-restore' },
          { text: 'Troubleshooting', link: '/guide/troubleshooting' }
        ]
      },
      {
        text: 'Configuration',
        items: [
          { text: 'Network & remote access', link: '/config/network' },
          { text: 'Settings reference', link: '/config/settings' },
          { text: 'AI agents (MCP)', link: '/config/ai-agents' }
        ]
      },
      {
        text: 'Modules',
        items: [
          { text: 'Overview', link: '/modules/' },
          { text: 'Trading Journal', link: '/modules/journal' },
          { text: 'Market data & backtesting', link: '/modules/market-data' },
          { text: 'Portfolios & wealth', link: '/modules/portfolio' },
          { text: 'News & research', link: '/modules/news-research' },
          { text: 'Notes & organization', link: '/modules/productivity' }
        ]
      }
    ],

    socialLinks: [{ icon: 'github', link: 'https://github.com/G-OTW/OpenTraderWorld' }],

    search: { provider: 'local' },

    editLink: {
      pattern: 'https://github.com/G-OTW/OpenTraderWorld/edit/master/docs/:path',
      text: 'Suggest a change to this page'
    },

    outline: { level: [2, 3] },

    footer: {
      message: 'Free for personal & non-commercial use. Source-available.'
    }
  }
});
