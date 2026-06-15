const invoke = window.__TAURI__.core.invoke;

const THEME_KEY = 'bogominer_theme';

let appState = null;
let runtimeStats = null;
let cpuTarget = 1.0;
let statsTimer = null;
let leaderboardTimer = null;
let gpuSettings = null;

const $ = (id) => document.getElementById(id);

const TIERS = [
  { name: 'recruit', min: 0, color: '#8a847a' },
  { name: 'cadet', min: 1e9, color: '#5b8fb4' },
  { name: 'operator', min: 1e10, color: '#4a9e6a' },
  { name: 'engineer', min: 1e11, color: '#da7656' },
  { name: 'architect', min: 1e12, color: '#8b5cf6' },
  { name: 'overseer', min: 1e13, color: '#c08a2e' },
  { name: 'luminary', min: 1e14, color: '#d4493e' },
];

const PRESTIGE_STEP = 1e14;
const XP_PER = 10000;

function escapeHtml(s) {
  return String(s ?? '').replace(
    /[&<>"']/g,
    (c) =>
      ({
        '&': '&amp;',
        '<': '&lt;',
        '>': '&gt;',
        '"': '&quot;',
        "'": '&#39;',
      })[c],
  );
}

function fmtCompact(n) {
  n = Number(n) || 0;
  if (n < 1000) return String(Math.round(n));
  if (n < 1_000_000) return `${(n / 1_000).toFixed(n < 10_000 ? 1 : 0)}K`;
  if (n < 1_000_000_000)
    return `${(n / 1_000_000).toFixed(n < 10_000_000 ? 2 : 1)}M`;
  if (n < 1e12) return `${(n / 1e9).toFixed(n < 1e10 ? 2 : 1)}B`;
  return `${(n / 1e12).toFixed(2)}T`;
}

function fmtCommas(n) {
  return Math.floor(Number(n) || 0).toLocaleString('en-US');
}

function fmtXp(n) {
  n = Number(n) || 0;
  if (n >= 1e6) return `${(n / 1e6).toFixed(n >= 1e7 ? 0 : 1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(n >= 1e4 ? 0 : 1)}k`;
  return String(Math.round(n));
}

function rankInfo(shuffles) {
  shuffles = Number(shuffles) || 0;
  let idx = 0;
  for (let i = 0; i < TIERS.length; i++) {
    if (shuffles >= TIERS[i].min) idx = i;
  }

  const luminaryIdx = TIERS.length - 1;
  const xp = Math.floor(shuffles / XP_PER);

  if (idx < luminaryIdx) {
    const cur = TIERS[idx].min;
    const nxt = TIERS[idx + 1].min;
    return {
      idx,
      name: TIERS[idx].name,
      color: TIERS[idx].color,
      stars: 0,
      pct: Math.max(0, Math.min(100, ((shuffles - cur) / (nxt - cur)) * 100)),
      nextLabel: TIERS[idx + 1].name,
      remXp: Math.floor((nxt - shuffles) / XP_PER),
      xp,
    };
  }

  const stars = Math.floor((shuffles - TIERS[luminaryIdx].min) / PRESTIGE_STEP);
  const base = TIERS[luminaryIdx].min + stars * PRESTIGE_STEP;
  return {
    idx: luminaryIdx,
    name: TIERS[luminaryIdx].name,
    color: TIERS[luminaryIdx].color,
    stars,
    pct: Math.max(0, Math.min(100, ((shuffles - base) / PRESTIGE_STEP) * 100)),
    nextLabel: `✦${stars + 1}`,
    remXp: Math.floor((base + PRESTIGE_STEP - shuffles) / XP_PER),
    xp,
  };
}

function applyTheme(theme) {
  const t = theme === 'dark' ? 'dark' : 'light';
  document.documentElement.dataset.theme = t;
  $('theme-icon-sun').style.display = t === 'dark' ? 'inline' : 'none';
  $('theme-icon-moon').style.display = t === 'dark' ? 'none' : 'inline';
  localStorage.setItem(THEME_KEY, t);
}

function showError(id, msg) {
  const el = $(id);
  el.textContent = msg || '';
}

function setConnectError(msg) {
  const el = $('connect-error');
  el.textContent = msg || '';
  el.classList.toggle('is-shown', Boolean(msg));
}

function updateAccountView() {
  const account = appState?.account;
  const ready = Boolean(account?.ready);

  $('onboard-section').hidden = ready;
  $('dashboard-section').hidden = !ready;

  if (!ready) return;

  $('account-name').textContent = account.nickname || 'pending';

  const uuidChip = $('uuid-chip');
  if (account.uuid) {
    uuidChip.textContent = `uuid ${account.uuid.slice(0, 8)}…`;
    uuidChip.classList.add('good');
  } else {
    uuidChip.textContent = 'waiting for server uuid';
    uuidChip.classList.remove('good');
  }

  const codeChip = $('code-chip');
  if (account.hasRecoveryCode) {
    codeChip.textContent = 'recovery code saved';
    codeChip.classList.add('good');
  } else {
    codeChip.textContent = 'recovery code pending';
    codeChip.classList.remove('good');
  }
}

function updateControls(running) {
  $('idle-controls').hidden = running;
  $('active-controls').hidden = !running;
  $('console-stats').hidden = !running;
}

function renderStats(s) {
  runtimeStats = s;
  updateControls(s.running);

  const gpu = s.backend === 'gpu';
  $('cpu-options-live').style.display = gpu ? 'none' : '';
  document.querySelector('.live-pill').textContent = gpu
    ? 'contributing live · gpu'
    : 'contributing live';
  if (gpu && !s.running && s.gpuStatus && s.gpuStatus.startsWith('error')) {
    setConnectError(`gpu worker stopped — ${s.gpuStatus}`);
  }

  $('stat-rate').textContent = `${fmtCompact(s.rate)}/s`;
  $('stat-threads').textContent = gpu ? 'GPU' : String(s.solverThreads || 0);
  $('stat-session').textContent = fmtCompact(s.sessionShuffles);
  $('stat-lifetime').textContent = fmtCompact(s.lifetimeShuffles);
  $('stat-tick-best').textContent =
    s.tickBest >= 0 ? `${s.tickBest} / 25` : '— / 25';
  $('stat-session-best').textContent =
    s.sessionBest >= 0 ? `${s.sessionBest} / 25` : '— / 25';
  $('stat-alltime-best').textContent =
    s.allTimeBest > 0 ? `${s.allTimeBest} / 25` : '— / 25';

  const last5 = $('stat-last5');
  if (!s.last5 || !s.last5.length) {
    last5.innerHTML = `<span class="l5-dash">—</span>`;
  } else {
    const peak = Math.max(...s.last5);
    let usedPeak = false;
    last5.innerHTML = s.last5
      .map((v) => {
        const top = v === peak && !usedPeak;
        if (top) usedPeak = true;
        return `<span class="l5-pill${top ? ' l5-top' : ''}">${v}</span>`;
      })
      .join('');
  }

  renderTier(s.lifetimeShuffles);
}

function renderTier(total) {
  const r = rankInfo(total);
  const name = r.stars > 0 ? `${r.name} ✦${r.stars}` : r.name;

  $('dashboard-section').style.setProperty('--tier-color', r.color);
  $('tier-name').textContent = name;
  $('tier-xp').textContent = `${fmtCommas(r.xp)} xp`;
  $('tier-bar-fill').style.width = `${r.pct}%`;
  $('tier-next').innerHTML =
    `<strong>${fmtXp(r.remXp)} xp</strong> to ${escapeHtml(r.nextLabel)}`;
  $('tier-pips').innerHTML = TIERS.map(
    (_, i) => `<span class="tier-pip${i <= r.idx ? ' reached' : ''}"></span>`,
  ).join('');
}

function renderLeaderboard(entries) {
  const lb = $('leaderboard');
  if (!entries || !entries.length) {
    lb.innerHTML = `<div class="lb-empty">no contributors yet.</div>`;
    return;
  }

  const me = appState?.account?.nickname || '';
  let html = `
    <div class="lb-row lb-row-head">
      <span class="lb-pos">#</span>
      <span class="lb-name">name</span>
      <span class="lb-total">shuffles</span>
    </div>
  `;

  entries.forEach((entry, i) => {
    const pos =
      i === 0 ? '🥇' : i === 1 ? '🥈' : i === 2 ? '🥉' : String(i + 1);
    const r = rankInfo(entry.total || 0);
    const tier = r.stars > 0 ? `${r.name} ✦${r.stars}` : r.name;
    const isMe = entry.nickname === me;
    html += `
      <div class="lb-row${isMe ? ' is-me' : ''}">
        <span class="lb-pos">${pos}</span>
        <span class="lb-name">
          <span class="lb-nick">${escapeHtml(entry.nickname)}</span>
          ${isMe ? `<span class="lb-me-tag">you</span>` : ''}
          <span class="lb-tier" style="color:${r.color}">${escapeHtml(tier)}</span>
        </span>
        <span class="lb-total">${fmtCompact(entry.total || 0)}</span>
      </div>
    `;
  });

  lb.innerHTML = html;
}

function setCpuTarget(next) {
  cpuTarget = Number(next);
  document.querySelectorAll('.cpu-opt').forEach((btn) => {
    btn.classList.toggle(
      'active',
      Math.abs(Number(btn.dataset.cpu) - cpuTarget) < 0.001,
    );
  });
  invoke('set_cpu_target', { cpuTarget }).catch((err) =>
    setConnectError(String(err)),
  );
}

async function refreshAppState() {
  appState = await invoke('get_app_state');
  $('version-label').textContent = `bogominer ${appState.version}`;
  updateAccountView();
}

async function refreshStats() {
  const s = await invoke('get_runtime_stats');
  renderStats(s);
  const oldUuid = appState?.account?.uuid;
  const oldNick = appState?.account?.nickname;
  const oldCode = appState?.account?.hasRecoveryCode;
  await refreshAppState();
  if (
    oldUuid !== appState.account.uuid ||
    oldNick !== appState.account.nickname ||
    oldCode !== appState.account.hasRecoveryCode
  ) {
    updateAccountView();
  }
}

async function refreshLeaderboard() {
  try {
    const entries = await invoke('get_leaderboard');
    renderLeaderboard(entries);
  } catch {
    $('leaderboard').innerHTML =
      `<div class="lb-empty">could not load leaderboard.</div>`;
  }
}

function renderGpuSettings() {
  if (!gpuSettings) return;
  $('gpu-toggle').checked = gpuSettings.enabled;
  if (document.activeElement !== $('gpu-path')) {
    $('gpu-path').value = gpuSettings.configuredPath || '';
  }
  const note = $('gpu-status-note');
  if (gpuSettings.available) {
    note.textContent = `worker found: ${gpuSettings.resolvedPath}`;
  } else if (gpuSettings.configuredPath) {
    note.textContent = 'configured path does not exist.';
  } else {
    note.textContent =
      'worker not present yet — it will be downloaded automatically when you enable gpu acceleration.';
  }
}

async function refreshGpuSettings() {
  try {
    gpuSettings = await invoke('get_gpu_settings');
    renderGpuSettings();
  } catch (err) {
    showError('gpu-error', String(err));
  }
}

async function loadContributors() {
  try {
    const contributors = await invoke('get_contributors');
    const root = $('contributors');
    if (!contributors.length) {
      root.innerHTML = `<div class="contributors-empty">no contributors found.</div>`;
      return;
    }
    root.innerHTML = contributors
      .map(
        (c) => `
      <button class="contributor" data-url="${escapeHtml(c.webUrl)}" title="${escapeHtml(c.name)}">
        <img src="${escapeHtml(c.avatarUrl)}" />
        <span class="contributor-tip">${escapeHtml(c.name)}</span>
      </button>
    `,
      )
      .join('');
  } catch {
    $('contributors').innerHTML =
      `<div class="contributors-empty">could not load contributors: ${escapeHtml(String(err))}</div>`;
  }
}

function wireEvents() {
  $('theme-toggle').addEventListener('click', () => {
    applyTheme(
      document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark',
    );
  });

  document.querySelectorAll('.tab').forEach((tab) => {
    tab.addEventListener('click', () => {
      document
        .querySelectorAll('.tab')
        .forEach((t) => t.classList.remove('active'));
      document
        .querySelectorAll('.tab-pane')
        .forEach((p) => p.classList.remove('active'));
      tab.classList.add('active');
      $(`tab-${tab.dataset.tab}`).classList.add('active');
    });
  });

  $('save-new').addEventListener('click', async () => {
    showError('new-error', '');
    try {
      console.log('[account:create] nickname =', $('new-nick').value);
      appState = await invoke('save_new_account', {
        req: { nickname: $('new-nick').value },
      });
      console.log('[account:create] response =', appState);
      updateAccountView();
    } catch (err) {
      console.error('[account:create] failed', err);
      showError('new-error', String(err));
    }
  });

  $('save-existing').addEventListener('click', async () => {
    showError('existing-error', '');
    try {
      console.log('[account:login] code =', $('existing-code').value);
      appState = await invoke('save_existing_account', {
        req: { recoveryCode: $('existing-code').value },
      });
      console.log('[account:login] response =', appState);
      updateAccountView();
    } catch (err) {
      console.error('[account:login] failed', err);
      showError('existing-error', String(err));
    }
  });

  $('start-btn').addEventListener('click', async () => {
    setConnectError('');
    try {
      await invoke('start_mining', { cpuTarget });
      await refreshStats();
    } catch (err) {
      setConnectError(String(err));
    }
  });

  $('stop-btn').addEventListener('click', async () => {
    await invoke('stop_mining');
    await refreshStats();
  });

  document.querySelectorAll('.cpu-opt').forEach((btn) => {
    btn.addEventListener('click', () => setCpuTarget(btn.dataset.cpu));
  });

  $('switch-account').addEventListener('click', async () => {
    appState = await invoke('clear_account');
    $('new-nick').value = '';
    $('existing-code').value = '';
    showError('new-error', '');
    showError('existing-error', '');
    updateAccountView();
    await refreshStats();
  });

  $('settings-btn').addEventListener('click', () => {
    $('settings-modal').classList.add('is-open');
    $('settings-modal').setAttribute('aria-hidden', 'false');
    refreshGpuSettings();
  });

  $('gpu-toggle').addEventListener('change', async (e) => {
    showError('gpu-error', '');
    try {
      if (e.target.checked && gpuSettings && !gpuSettings.available) {
        $('gpu-status-note').textContent =
          'downloading bogo-turbo worker (~2 MB)…';
        gpuSettings = await invoke('download_gpu_worker');
      }
      gpuSettings = await invoke('set_gpu_enabled', {
        enabled: e.target.checked,
      });
      renderGpuSettings();
    } catch (err) {
      e.target.checked = !e.target.checked;
      renderGpuSettings();
      showError('gpu-error', String(err));
    }
  });

  $('gpu-path-save').addEventListener('click', async () => {
    showError('gpu-error', '');
    try {
      gpuSettings = await invoke('set_gpu_worker_path', {
        path: $('gpu-path').value,
      });
      renderGpuSettings();
    } catch (err) {
      showError('gpu-error', String(err));
    }
  });

  $('settings-close').addEventListener('click', closeSettings);
  $('settings-modal').addEventListener('click', (e) => {
    if (e.target.id === 'settings-modal') closeSettings();
  });

  $('contributors').addEventListener('click', async (e) => {
    const btn = e.target.closest('.contributor');
    if (!btn) return;
    const url = btn.dataset.url;
    if (!url) return;
    await invoke('open_external', { url }).catch(() => {});
  });
}

function closeSettings() {
  $('settings-modal').classList.remove('is-open');
  $('settings-modal').setAttribute('aria-hidden', 'true');
}

async function boot() {
  applyTheme(localStorage.getItem(THEME_KEY) || 'light');
  wireEvents();
  await refreshAppState();
  await invoke('prime_lifetime_stats').catch(() => {});
  await refreshStats();
  await refreshGpuSettings();
  await refreshLeaderboard();
  loadContributors();

  statsTimer = setInterval(refreshStats, 500);
  leaderboardTimer = setInterval(refreshLeaderboard, 5000);
}

boot();
