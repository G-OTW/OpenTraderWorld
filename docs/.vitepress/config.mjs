import { defineConfig } from 'vitepress';

export default defineConfig({
  title: 'OpenTraderWorld',
  description:
    'Self-hosted, modular platform for traders and investors: journal, market data, backtesting, portfolios and more, on your own machine.',
  base: '/OpenTraderWorld/',
  cleanUrls: true,
  lastUpdated: true,
  srcExclude: ['README.md'],

  head: [['link', { rel: 'icon', type: 'image/svg+xml', href: '/OpenTraderWorld/favicon.svg' }]],

  themeConfig: {
    nav: [
      { text: 'Guide', link: '/guide/introduction', activeMatch: '/guide/' },
      { text: 'Configuration', link: '/config/network', activeMatch: '/config/' },
      { text: 'Modules', link: '/modules/', activeMatch: '/modules/' },
      {
        text: 'opentraderworld.com',
        items: [
          { text: 'Project website', link: 'https://opentraderworld.com' },
          { text: 'Live demo', link: 'https://demo.opentraderworld.com' },
          { text: 'Community Docs', link: 'https://opentraderworld.com/docs' },
          { text: 'Suggestions & polls', link: 'https://opentraderworld.com/suggestions' }
        ]
      }
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
          { text: 'Troubleshooting', link: '/guide/troubleshooting' },
          { text: 'Demo mode', link: '/guide/demo' }
        ]
      },
      {
        text: 'Configuration',
        items: [
          { text: 'Network & remote access', link: '/config/network' },
          { text: 'Account security', link: '/config/security' },
          { text: 'Settings reference', link: '/config/settings' },
          { text: 'Data connectors', link: '/config/connectors' },
          { text: 'Broker accounts', link: '/config/brokers' },
          { text: 'AI agents (MCP)', link: '/config/ai-agents' },
          { text: 'External control (chat)', link: '/config/external-control' }
        ]
      },
      {
        text: 'Modules',
        items: [
          { text: 'Overview', link: '/modules/' },
          { text: 'Dashboard & navigation', link: '/modules/dashboard' },
          { text: 'Trading Journal', link: '/modules/journal' },
          { text: 'Market data & backtesting', link: '/modules/market-data' },
          { text: 'Portfolios & wealth', link: '/modules/portfolio' },
          { text: 'News & research', link: '/modules/news-research' },
          { text: 'Notes & organization', link: '/modules/productivity' },
          { text: 'Automator (workflows)', link: '/modules/automator' },
          { text: 'Agent (AI assistant)', link: '/modules/agent' }
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
      message:
        'Free for personal & non-commercial use. Source-available. <a href="https://opentraderworld.com">opentraderworld.com</a> · <a href="https://demo.opentraderworld.com">Live demo</a> · <a href="https://opentraderworld.com/suggestions">Suggest a feature</a>'
    }
  }
});
