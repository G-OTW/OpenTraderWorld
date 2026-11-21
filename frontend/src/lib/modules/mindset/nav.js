/** The module's fixed top menu, shared by the Mindset pages so the links (and their
 *  order) are declared once rather than repeated per route. */
export const NAV_LINKS = [
  { href: '/mindset', icon: 'check-square', key: 'mindset.nav.checkin' },
  { href: '/mindset/templates', icon: 'clipboard-list', key: 'mindset.nav.templates' },
  { href: '/mindset/dashboard', icon: 'bar-chart', key: 'mindset.nav.dashboard' },
  { href: '/mindset/history', icon: 'history', key: 'mindset.nav.history' }
];

export { mindsetApi, PHASES, KINDS, kindOf, isAnswered, optionPresets } from './api.js';
export { SCORE_MODES, scoreMode } from './scoring.js';
