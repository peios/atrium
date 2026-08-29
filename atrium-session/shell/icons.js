// Stroked line icons, 16px default. From the Peios design system's shared
// icon set; one path per glyph, drawn with currentColor.
const P = {
  server:   'M3 4h18v6H3zM3 14h18v6H3zM6 7h.01M6 17h.01',
  shield:   'M12 3l8 3v6c0 4.5-3.2 8.4-8 9-4.8-.6-8-4.5-8-9V6z',
  globe:    'M12 3a9 9 0 100 18 9 9 0 000-18zM3 12h18M12 3c2.5 3 2.5 15 0 18M12 3c-2.5 3-2.5 15 0 18',
  cert:     'M4 4h16v12H4zM8 20l4-2 4 2M8 9h8M8 13h5',
  folder:   'M3 6l3-2h4l2 2h9v12H3z',
  disk:     'M3 6l2-2h14l2 2v12l-2 2H5l-2-2zM7 8h10M7 12h10M7 16h6',
  cpu:      'M7 4h10v4h-10zM7 16h10v4h-10zM4 7v10M20 7v10M4 7h3M4 11h3M4 15h3M17 7h3M17 11h3M17 15h3M9 8v8M12 8v8M15 8v8',
  box:      'M3 7l9-4 9 4-9 4zM3 7v10l9 4M21 7v10l-9 4M3 7l9 4M21 7l-9 4',
  www:      'M3 5h18v3H3zM3 5v14h18V5M6 12h12M6 15h8',
  arrows:   'M4 7h12l-3-3M20 17H8l3 3',
  key:      'M15 8a5 5 0 11-4.9 6H4v-3h3v-3h3l.1.1A5 5 0 0115 8zM17 11h.01',
  archive:  'M3 5h18v4H3zM5 9v11h14V9M10 13h4',
  chart:    'M4 4v16h16M8 16l3-5 3 3 5-7',
  mail:     'M3 6h18v12H3zM3 6l9 7 9-7',
  settings: 'M12 9a3 3 0 100 6 3 3 0 000-6zM19 12c0 .4 0 .8-.1 1.2l2 1.5-2 3.5-2.4-1a7 7 0 01-2 1.2l-.4 2.6h-4l-.4-2.6a7 7 0 01-2-1.2l-2.4 1-2-3.5 2.1-1.5a7 7 0 010-2.4L3.2 9.3l2-3.5 2.4 1a7 7 0 012-1.2L10 3h4l.4 2.6a7 7 0 012 1.2l2.4-1 2 3.5-2.1 1.5c.1.4.1.8.1 1.2z',
  search:   'M10 4a6 6 0 104.47 10.03L20 20l0 0M10 4a6 6 0 014.47 10.03',
  bell:     'M6 8a6 6 0 1112 0v4l2 4H4l2-4zM9 18a3 3 0 006 0',
  moon:     'M21 12.8A8 8 0 1111.2 3a6 6 0 009.8 9.8z',
  sun:      'M12 6a6 6 0 100 12 6 6 0 000-12zM12 2v2M12 20v2M2 12h2M20 12h2M5 5l1 1M18 18l1 1M19 5l-1 1M6 18l-1 1',
  plus:     'M12 5v14M5 12h14',
  close:    'M6 6l12 12M18 6L6 18',
  check:    'M5 12l4 4 10-10',
  chevR:    'M9 6l6 6-6 6',
  chevD:    'M6 9l6 6 6-6',
  chevU:    'M6 15l6-6 6 6',
  more:     'M5 12h.01M12 12h.01M19 12h.01',
  filter:   'M4 5h16l-6 8v6l-4-2v-4z',
  refresh:  'M4 10a8 8 0 0114-5l2-2v6h-6l2-2a5 5 0 00-9 3M20 14a8 8 0 01-14 5l-2 2v-6h6l-2 2a5 5 0 009-3',
  terminal: 'M4 5h16v14H4zM7 9l3 3-3 3M13 15h5',
  download: 'M12 4v12m-4-4l4 4 4-4M4 20h16',
  upload:   'M12 20V8m-4 4l4-4 4 4M4 4h16',
  play:     'M6 4l14 8-14 8z',
  pause:    'M7 4h3v16H7zM14 4h3v16h-3z',
  restart:  'M4 10a8 8 0 1114-5M20 4v6h-6',
  power:    'M12 4v8M6 7a8 8 0 1012 0',
  stop:     'M6 6h12v12H6z',
  lock:     'M6 10V7a6 6 0 0112 0v3M4 10h16v10H4z',
  unlock:   'M6 10V7a6 6 0 0111.2-3M4 10h16v10H4z',
  user:     'M12 12a4 4 0 100-8 4 4 0 000 8zM4 20c0-4 3.6-6 8-6s8 2 8 6',
  users:    'M9 12a4 4 0 100-8 4 4 0 000 8zM17 13a3 3 0 100-6 3 3 0 000 6zM2 20c0-3.5 3.2-6 7-6s7 2.5 7 6M16 20c0-2.8 2-5 5-5',
  alert:    'M12 3l10 18H2zM12 10v5M12 18h.01',
  info:     'M12 3a9 9 0 100 18 9 9 0 000-18zM12 8h.01M11 12h1v5',
  clock:    'M12 3a9 9 0 100 18 9 9 0 000-18zM12 7v5l3 2',
  trash:    'M4 7h16M10 7V4h4v3M6 7v13h12V7M10 11v6M14 11v6',
  pencil:   'M3 21l4-1 12-12-3-3L4 17zM14 6l3 3',
  calendar: 'M3 6h18v14H3zM3 10h18M8 3v4M16 3v4',
  eye:      'M2 12s4-7 10-7 10 7 10 7-4 7-10 7-10-7-10-7zM12 9a3 3 0 100 6 3 3 0 000-6z',
  tree:     'M5 3v8M5 7h4v4M5 11v6h4M5 17v0M9 7h4M13 11h4',
  map:      'M3 6l6-2 6 2 6-2v14l-6 2-6-2-6 2zM9 4v14M15 6v14',
  package:  'M3 7l9-4 9 4v10l-9 4-9-4zM12 3v18M3 7l9 4 9-4',
  cloud:    'M7 18a5 5 0 01-1-9.9A6 6 0 0118 9a4 4 0 01-2 7.5',
  flag:     'M4 3v18M4 5h10l2 2h4v7h-6l-2-2H4',
  plug:     'M9 4v4M15 4v4M6 8h12v5a6 6 0 01-12 0zM12 19v3',
  pin:      'M14 3l7 7-3 1-4 4-1 5-9-9 5-1 4-4z',
  bug:      'M8 4l2 2M16 4l-2 2M9 7h6a3 3 0 013 3v1H6v-1a3 3 0 013-3zM6 11v3a6 6 0 0012 0v-3M3 13h3M18 13h3M4 18l3-2M20 18l-3-2M4 8l3 1M20 8l-3 1',
  fileText: 'M6 3h9l4 4v14H6zM14 3v5h5M9 12h7M9 16h7M9 8h3',
  file:     'M6 3h9l4 4v14H6zM14 3v5h5',
  code:     'M8 8l-4 4 4 4M16 8l4 4-4 4M14 5l-4 14',
  logout:   'M10 4H5v16h5M14 8l4 4-4 4M18 12H9',
};

const NS = 'http://www.w3.org/2000/svg';

/** An <svg> element for a named glyph; an empty span if the name is unknown. */
export function icon(name, size = 16, stroke = 1.6, className = '') {
  const d = P[name];
  if (!d) return document.createElement('span');
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('width', size); svg.setAttribute('height', size);
  svg.setAttribute('viewBox', '0 0 24 24'); svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor'); svg.setAttribute('stroke-width', stroke);
  svg.setAttribute('stroke-linecap', 'round'); svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  if (className) svg.setAttribute('class', className);
  const path = document.createElementNS(NS, 'path');
  path.setAttribute('d', d);
  svg.append(path);
  return svg;
}

export const ICON_NAMES = Object.keys(P);
