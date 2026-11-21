import { readFileSync } from 'node:fs';
import { defineConfig } from 'vitepress';

const LANGS = ['en', 'de', 'es', 'fr', 'it', 'pt', 'zh'];
const tr = Object.fromEntries(
  LANGS.map((l) => [l, JSON.parse(readFileSync(new URL(`./i18n/${l}.json`, import.meta.url), 'utf8'))])
);

// Default theme labels per locale. English keeps VitePress' own defaults.
const UI = {
  de: {
    outline: 'Auf dieser Seite', prev: 'Vorherige Seite', next: 'Nächste Seite',
    lastUpdated: 'Zuletzt aktualisiert', darkMode: 'Erscheinungsbild', menu: 'Menü',
    top: 'Nach oben', lang: 'Sprache ändern'
  },
  es: {
    outline: 'En esta página', prev: 'Página anterior', next: 'Página siguiente',
    lastUpdated: 'Última actualización', darkMode: 'Apariencia', menu: 'Menú',
    top: 'Volver arriba', lang: 'Cambiar idioma'
  },
  fr: {
    outline: 'Sur cette page', prev: 'Page précédente', next: 'Page suivante',
    lastUpdated: 'Dernière mise à jour', darkMode: 'Apparence', menu: 'Menu',
    top: 'Retour en haut', lang: 'Changer de langue'
  },
  it: {
    outline: 'In questa pagina', prev: 'Pagina precedente', next: 'Pagina successiva',
    lastUpdated: 'Ultimo aggiornamento', darkMode: 'Aspetto', menu: 'Menu',
    top: 'Torna su', lang: 'Cambia lingua'
  },
  pt: {
    outline: 'Nesta página', prev: 'Página anterior', next: 'Próxima página',
    lastUpdated: 'Última atualização', darkMode: 'Aparência', menu: 'Menu',
    top: 'Voltar ao topo', lang: 'Mudar idioma'
  },
  zh: {
    outline: '页面导航', prev: '上一页', next: '下一页',
    lastUpdated: '最后更新于', darkMode: '主题', menu: '菜单',
    top: '返回顶部', lang: '多语言'
  }
};

// Local search box and modal per locale, keyed like `locales` (English is `root`, the default).
const SEARCH = {
  de: ['Suchen', 'Detaillierte Liste anzeigen', 'Suche zurücksetzen', 'Suche schließen', 'Keine Ergebnisse für', 'auswählen', 'navigieren', 'schließen'],
  es: ['Buscar', 'Mostrar lista detallada', 'Restablecer búsqueda', 'Cerrar búsqueda', 'No hay resultados para', 'seleccionar', 'navegar', 'cerrar'],
  fr: ['Rechercher', 'Afficher la liste détaillée', 'Réinitialiser la recherche', 'Fermer la recherche', 'Aucun résultat pour', 'sélectionner', 'naviguer', 'fermer'],
  it: ['Cerca', 'Mostra elenco dettagliato', 'Reimposta ricerca', 'Chiudi ricerca', 'Nessun risultato per', 'seleziona', 'naviga', 'chiudi'],
  pt: ['Pesquisar', 'Mostrar lista detalhada', 'Limpar pesquisa', 'Fechar pesquisa', 'Nenhum resultado para', 'selecionar', 'navegar', 'fechar'],
  zh: ['搜索文档', '显示详细列表', '清除查询条件', '关闭搜索', '无法找到相关结果', '选择', '切换', '关闭']
};
const searchLocales = Object.fromEntries(
  Object.entries(SEARCH).map(([l, [button, details, reset, back, none, select, navigate, close]]) => [
    l,
    {
      translations: {
        button: { buttonText: button, buttonAriaLabel: button },
        modal: {
          displayDetails: details,
          resetButtonTitle: reset,
          backButtonTitle: back,
          noResultsText: none,
          footer: { selectText: select, navigateText: navigate, closeText: close }
        }
      }
    }
  ])
);

const labelsFor = (lang) => {
  const u = UI[lang];
  if (!u) return {};
  return {
    docFooter: { prev: u.prev, next: u.next },
    lastUpdated: { text: u.lastUpdated },
    darkModeSwitchLabel: u.darkMode,
    sidebarMenuLabel: u.menu,
    returnToTopLabel: u.top,
    langMenuLabel: u.lang
  };
};

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
          { text: s.trading, link: `${p}/config/trading` },
          { text: s.paperTrading, link: `${p}/config/paper-trading` },
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
          { text: s.robots, link: `${p}/modules/robots` },
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
    editLink: {
      pattern: 'https://github.com/G-OTW/OpenTraderWorld/edit/master/docs/:path',
      text: t.ui.editLink
    },
    outline: UI[lang] ? { level: [2, 4], label: UI[lang].outline } : { level: [2, 4] },
    footer: { message: t.ui.footer },
    ...labelsFor(lang)
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

  // Read at build time from the top level only (it decides whether the index is built), and
  // not repeated per locale: a locale's `search` would replace this one, translations included.
  themeConfig: {
    search: { provider: 'local', options: { locales: searchLocales } }
  }
});
