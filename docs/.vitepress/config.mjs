import { readFileSync } from 'node:fs';
import { defineConfig } from 'vitepress';

const LANGS = ['en', 'de', 'es', 'fr', 'it', 'pt', 'zh'];
const tr = Object.fromEntries(
  LANGS.map((l) => [l, JSON.parse(readFileSync(new URL(`./i18n/${l}.json`, import.meta.url), 'utf8'))])
);

const themeFor = (lang) => {
  const t = tr[lang];
  const p = lang === 'en' ? '' : `/${lang}`;
  const s = t.sidebar;
  return {
    nav: [
      { text: t.nav.guide, link: `${p}/guide/introduction`, activeMatch: `${p}/guide/` },
      { text: t.nav.config, link: `${p}/config/network`, activeMatch: `${p}/config/` },
      { text: t.nav.modules, link: `${p}/modules/`, activeMatch: `${p}/modules/` },
      {
        text: t.nav.site,
        items: [
          { text: t.nav.website, link: 'https://opentraderworld.com' },
          { text: t.nav.demo, link: 'https://demo.opentraderworld.com' },
          { text: t.nav.community, link: 'https://opentraderworld.com/docs' },
          { text: t.nav.suggestions, link: 'https://opentraderworld.com/suggestions' }
        ]
      }
    ],
    sidebar: [
      {
        text: s.gettingStarted,
        items: [
          { text: s.introduction, link: `${p}/guide/introduction` },
          { text: s.docker, link: `${p}/guide/docker` },
          { text: s.install, link: `${p}/guide/install` },
          { text: s.firstSteps, link: `${p}/guide/first-steps` },
          { text: s.updating, link: `${p}/guide/updating` },
          { text: s.backup, link: `${p}/guide/backup-restore` },
          { text: s.troubleshooting, link: `${p}/guide/troubleshooting` },
          { text: s.demoMode, link: `${p}/guide/demo` }
        ]
      },
      {
        text: s.online,
        items: [{ text: s.whenItBreaks, link: `${p}/guide/hosting/when-it-breaks` }]
      },
      {
        text: s.configuration,
        items: [
          { text: s.network, link: `${p}/config/network` },
          { text: s.security, link: `${p}/config/security` },
          { text: s.social, link: `${p}/config/social-login` },
          { text: s.settings, link: `${p}/config/settings` },
          { text: s.connectors, link: `${p}/config/connectors` },
          { text: s.brokers, link: `${p}/config/brokers` },
          { text: s.aiAgents, link: `${p}/config/ai-agents` },
          { text: s.externalControl, link: `${p}/config/external-control` },
          { text: s.voice, link: `${p}/config/voice` }
        ]
      },
      {
        text: s.modules,
        items: [
          { text: s.overview, link: `${p}/modules/` },
          { text: s.dashboard, link: `${p}/modules/dashboard` },
          { text: s.journal, link: `${p}/modules/journal` },
          { text: s.marketData, link: `${p}/modules/market-data` },
          { text: s.fundamentals, link: `${p}/modules/fundamentals` },
          { text: s.portfolio, link: `${p}/modules/portfolio` },
          { text: s.news, link: `${p}/modules/news-research` },
          { text: s.productivity, link: `${p}/modules/productivity` },
          { text: s.automator, link: `${p}/modules/automator` },
          { text: s.agent, link: `${p}/modules/agent` }
        ]
      }
    ],
    socialLinks: [{ icon: 'github', link: 'https://github.com/G-OTW/OpenTraderWorld' }],
    search: { provider: 'local' },
    editLink: {
      pattern: 'https://github.com/G-OTW/OpenTraderWorld/edit/master/docs/:path',
      text: t.ui.editLink
    },
    outline: { level: [2, 3] },
    footer: { message: t.ui.footer }
  };
};

const locales = Object.fromEntries(
  LANGS.map((l) => [
    l === 'en' ? 'root' : l,
    {
      label: tr[l].label,
      lang: l === 'zh' ? 'zh-CN' : l,
      link: l === 'en' ? '/' : `/${l}/`,
      description: tr[l].description,
      themeConfig: themeFor(l)
    }
  ])
);

export default defineConfig({
  title: 'OpenTraderWorld',
  base: '/OpenTraderWorld/',
  cleanUrls: true,
  lastUpdated: true,
  srcExclude: ['README.md'],

  head: [['link', { rel: 'icon', type: 'image/svg+xml', href: '/OpenTraderWorld/favicon.svg' }]],

  locales,

  themeConfig: {
    socialLinks: [{ icon: 'github', link: 'https://github.com/G-OTW/OpenTraderWorld' }],
    search: { provider: 'local' }
  }
});
