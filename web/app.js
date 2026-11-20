// Interface du maître. Tout texte venu de la grappe est posé avec textContent : rien de ce
// qu'écrit un agent n'est jamais interprété comme du HTML.
'use strict';

const $ = (id) => document.getElementById(id);
const el = (tag, cls, text) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
};
const state = { token: null, tree: null, events: [], after: 0, selected: null, view: 'tree', releases: null, timer: null };

const STATUS = { starting: 'démarre', idle: 'disponible', working: 'au travail', suspended: 'en veille', disconnected: 'injoignable', stopped: 'arrêté' };
const KIND = { status: 'état', input: 'reçu', tool: 'outil', tool_error: 'refus', spawn: 'création', report: 'rapport', message: 'message', error: 'erreur', link: 'lien' };

async function api(path, options = {}) {
  const response = await fetch(path, {
    ...options,
    headers: { Authorization: 'Bearer ' + state.token, ...(options.body ? { 'Content-Type': 'application/json' } : {}) },
    cache: 'no-store',
  });
  if (response.status === 401) { logout('Jeton refusé.'); throw new Error('401'); }
  if (!response.ok) throw new Error((await response.json().catch(() => ({}))).error || 'erreur ' + response.status);
  return response.json();
}

function logout(message) {
  clearInterval(state.timer);
  sessionStorage.removeItem('talixio-token');
  localStorage.removeItem('talixio-token');
  state.token = null;
  $('app').hidden = true;
  $('login').hidden = false;
  $('login-error').hidden = !message;
  $('login-error').textContent = message || '';
  $('token').value = '';
  $('token').focus();
}

async function enter(token, remember) {
  state.token = token;
  await api('/api/tree');
  (remember ? localStorage : sessionStorage).setItem('talixio-token', token);
  $('login').hidden = true;
  $('app').hidden = false;
  Object.assign(state, { events: [], after: 0 });
  $('journal').replaceChildren();
  $('chat').replaceChildren();
  await refresh();
  state.timer = setInterval(refresh, 1500);
  api('/api/releases').then(renderReleases).catch(() => {});
}

async function refresh() {
  if (document.hidden) return;
  try {
    const [tree, log] = await Promise.all([api('/api/tree'), api('/api/activity?after=' + state.after + '&limit=2000')]);
    $('offline').hidden = true;
    state.tree = tree;
    renderTree();
    if (log.events.length) {
      state.after = log.events[log.events.length - 1].id;
      state.events.push(...log.events);
      if (state.events.length > 5000) state.events.splice(0, state.events.length - 5000);
      log.events.forEach(appendEvent);
    }
  } catch (error) {
    if (error.message !== '401') $('offline').hidden = false;
  }
}

const flat = (node, out = []) => { out.push(node); node.children.forEach((c) => flat(c, out)); return out; };
const hour = (ms) => new Date(ms).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
const count = (n) => n.toLocaleString('fr-FR');

function renderTree() {
  const all = flat(state.tree);
  const machines = new Set(all.map((a) => a.host)).size;
  $('summary').textContent = `${all.length} agent${all.length > 1 ? 's' : ''} · ${machines} machine${machines > 1 ? 's' : ''} · maître sur ${state.tree.host}`;
  const tally = {};
  all.forEach((a) => { tally[a.status] = (tally[a.status] || 0) + 1; });
  $('counts').replaceChildren(...Object.entries(tally).map(([s, n]) => el('span', 'pill ' + s, `${n} ${STATUS[s] || s}`)));
  if (!all.some((a) => a.id === state.selected)) state.selected = state.tree.id;
  $('agents').replaceChildren(...all.map((agent) => {
    const card = el('button', 'agent' + (agent.id === state.selected ? ' selected' : ''));
    card.type = 'button';
    card.style.marginLeft = Math.min(agent.depth, 6) * 22 + 'px';
    const top = el('div', 'top');
    top.append(el('span', 'name', agent.name), el('span', 'pill ' + agent.status, STATUS[agent.status] || agent.status), el('span', 'grow'), el('span', 'meta mono', agent.model));
    card.append(top, el('div', 'meta', `${agent.role} · ${agent.host} · ${agent.target === 'root' ? 'maître' : agent.target}`));
    const limit = agent.capabilities.max_tokens;
    if (limit) {
      const meter = el('div', 'meter');
      const bar = el('span');
      bar.style.width = Math.min(100, (agent.tokens_used / limit) * 100) + '%';
      meter.append(bar);
      card.append(meter);
    }
    card.addEventListener('click', () => { state.selected = agent.id; renderTree(); });
    return card;
  }));
  renderDetail(all.find((a) => a.id === state.selected));
}

function renderDetail(agent) {
  const caps = agent.capabilities;
  const rows = [
    ['Rôle', agent.role], ['État', STATUS[agent.status] || agent.status], ['Modèle', agent.model],
    ['Machine', `${agent.host} (${agent.os})`], ['Adresses', agent.ips.join(', ') || '—'], ['Déploiement', agent.target === 'root' ? 'maître' : agent.target],
    ['Tokens', count(agent.tokens_used) + (caps.max_tokens ? ' / ' + count(caps.max_tokens) : '')],
    ['Lecture', (caps.fs_read || []).join(', ') || 'aucune'], ['Écriture', (caps.fs_write || []).join(', ') || 'aucune'],
    ['Commandes', (caps.exec || []).map((r) => [r.program, ...(r.args || ['…'])].join(' ')).join(' ; ') || 'aucune'],
    ['Modèles', (caps.models || []).join(', ') || 'aucun'],
    ['Enfants', caps.spawn ? `${caps.spawn.max_children} au plus, ${caps.spawn.max_depth} niveau(x), sur ${caps.spawn.targets.join(', ')}` : 'ne peut pas en créer'],
    ['Démarré', new Date(agent.started_at_ms).toLocaleString('fr-FR')], ['Identifiant', agent.id],
  ];
  const list = el('dl');
  rows.forEach(([key, value]) => list.append(el('dt', '', key), el('dd', key === 'Identifiant' || key === 'Commandes' ? 'mono' : '', value)));
  const head = el('div', 'top');
  head.style.cssText = 'display:flex;align-items:center;gap:8px';
  head.append(el('h2', '', agent.name), el('span', 'pill ' + agent.status, STATUS[agent.status] || agent.status));
  const nodes = [head, list];
  if (agent.id !== state.tree.id) {
    const stop = el('button', 'danger', 'Arrêter cet agent et sa descendance');
    stop.type = 'button';
    stop.addEventListener('click', async () => {
      if (!confirm(`Arrêter « ${agent.name} » et toute sa descendance ?`)) return;
      await api(`/api/agents/${encodeURIComponent(agent.id)}/stop`, { method: 'POST', body: '{}' }).catch((e) => alert(e.message));
      refresh();
    });
    nodes.push(stop);
  }
  $('detail').replaceChildren(...nodes);
}

function appendEvent(event) {
  const row = el('div', 'event');
  const time = el('time', '', hour(event.ts_ms));
  row.append(time, el('span', 'who', event.agent_name), el('span', 'tag ' + event.kind, KIND[event.kind] || event.kind), el('span', 'text', event.text));
  row.dataset.search = (event.agent_name + ' ' + event.kind + ' ' + event.text).toLowerCase();
  row.hidden = !row.dataset.search.includes($('filter').value.trim().toLowerCase());
  const journal = $('journal');
  journal.prepend(row);
  while (journal.childElementCount > 2000) journal.lastElementChild.remove();

  if (state.tree && event.agent_id === state.tree.id && (event.kind === 'message' || event.kind === 'input')) {
    const mine = event.kind === 'input' && event.text.startsWith("de l'utilisateur : ");
    if (event.kind === 'input' && !mine) return;
    const bubble = el('div', 'bubble' + (mine ? ' me' : ''), mine ? event.text.slice("de l'utilisateur : ".length) : event.text);
    bubble.append(el('time', '', hour(event.ts_ms)));
    $('chat').append(bubble);
    if (state.view === 'chat') bubble.scrollIntoView({ block: 'end' });
  }
}

function renderReleases(data) {
  $('version').textContent = 'v' + data.version;
  $('running').textContent = `Ce qui a changé, version après version. Le maître tourne en v${data.version}.`;
  $('releases').replaceChildren(...data.releases.map((release) => {
    const current = release.version === data.version;
    const article = el('article', 'release' + (current ? ' current' : ''));
    const rail = el('div', 'rail');
    rail.append(el('i'), el('b'));
    const card = el('div', 'card');
    const head = el('div', 'head');
    head.append(el('span', 'mono muted', 'v' + release.version), el('h2', '', release.title));
    if (current) head.append(el('span', 'pill dark', 'version installée'));
    head.append(el('time', '', new Date(release.date).toLocaleDateString('fr-FR', { day: 'numeric', month: 'long', year: 'numeric' })));
    const list = el('ul');
    release.changes.forEach((change) => {
      const item = el('li', change.kind);
      item.append(el('span', 'tag ' + change.kind, change.kind === 'attention' ? 'à savoir' : change.kind), rich(change.text));
      list.append(item);
    });
    card.append(head, rich(release.summary, 'p'), list);
    article.append(rail, card);
    return article;
  }));
}

// `code` entre accents graves, sans jamais passer par du HTML.
function rich(text, tag = 'span') {
  const node = el(tag);
  text.split('`').forEach((part, index) => node.append(index % 2 ? el('code', '', part) : part));
  return node;
}

function show(view) {
  state.view = view;
  ['tree', 'chat', 'journal', 'releases'].forEach((name) => { $('view-' + name).hidden = name !== view; });
  document.querySelectorAll('nav [data-view]').forEach((b) => b.classList.toggle('active', b.dataset.view === view));
  if (view === 'chat') { window.scrollTo(0, document.body.scrollHeight); $('chat-text').focus(); }
}

document.querySelectorAll('nav [data-view]').forEach((b) => b.addEventListener('click', () => show(b.dataset.view)));
$('logout').addEventListener('click', () => logout());
$('filter').addEventListener('input', () => {
  const wanted = $('filter').value.trim().toLowerCase();
  for (const row of $('journal').children) row.hidden = !row.dataset.search.includes(wanted);
});
$('login-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  try { await enter($('token').value.trim(), $('remember').checked); }
  catch (error) { if (error.message !== '401') { $('login-error').hidden = false; $('login-error').textContent = 'Maître injoignable.'; } }
});
$('chat-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  const text = $('chat-text').value.trim();
  if (!text) return;
  try { await api('/api/messages', { method: 'POST', body: JSON.stringify({ text }) }); $('chat-text').value = ''; refresh(); }
  catch (error) { alert(error.message); }
});
$('chat-text').addEventListener('keydown', (event) => {
  if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); $('chat-form').requestSubmit(); }
});
document.addEventListener('visibilitychange', () => { if (!document.hidden && state.token) refresh(); });

const saved = sessionStorage.getItem('talixio-token') || localStorage.getItem('talixio-token');
if (saved) enter(saved, !!localStorage.getItem('talixio-token')).catch(() => logout());
else logout();
