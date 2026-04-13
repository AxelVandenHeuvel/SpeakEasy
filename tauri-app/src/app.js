let invoke, listen;

function initTauri() {
  if (window.__TAURI__) {
    invoke = window.__TAURI__.core.invoke;
    listen = window.__TAURI__.event.listen;
    setupBackendEvents();
    loadConfig();
    setupOnboarding();
  } else {
    setTimeout(initTauri, 100);
  }
}

// --- Onboarding (first-run permission wizard) ---

async function setupOnboarding() {
  try {
    const cfg = await invoke('get_config');
    if (!cfg.first_run_complete) {
      document.getElementById('onboarding').classList.remove('hidden');
      startPermissionPolling();
    }
  } catch (e) {}

  document.getElementById('grantMic').addEventListener('click', async () => {
    try { await invoke('request_microphone'); } catch (e) {}
  });

  document.getElementById('grantAx').addEventListener('click', async () => {
    try { await invoke('request_accessibility'); } catch (e) {}
  });

  document.getElementById('onboardingDone').addEventListener('click', async () => {
    try {
      const cfg = await invoke('get_config');
      cfg.first_run_complete = true;
      await invoke('save_config', { config: cfg });
      // Start hotkey listener now that permissions should be granted
      await invoke('start_hotkey_listener_cmd');
    } catch (e) {}
    document.getElementById('onboarding').classList.add('hidden');
  });
}

function startPermissionPolling() {
  const poll = async () => {
    if (document.getElementById('onboarding').classList.contains('hidden')) return;
    try {
      const p = await invoke('check_permissions');
      document.getElementById('permAx').classList.toggle('granted', p.accessibility);
      document.getElementById('permMic').classList.toggle('granted', p.microphone);
    } catch (e) {}
    setTimeout(poll, 1000);
  };
  poll();
}

// Tone switching
const toneDescs = {
  normal: 'Casual, natural writing. Good for chat, notes, and general use.',
  formal: 'Professional and polished. Good for emails, documents, and business communication.',
};

document.querySelectorAll('.tone-btn').forEach(btn => {
  btn.addEventListener('click', async () => {
    document.querySelectorAll('.tone-btn').forEach(b => b.classList.remove('active'));
    btn.classList.add('active');
    document.getElementById('toneDesc').textContent = toneDescs[btn.dataset.tone];

    // Save tone to config
    if (invoke) {
      try {
        const cfg = await invoke('get_config');
        cfg.tone = btn.dataset.tone;
        await invoke('save_config', { config: cfg });
      } catch (e) {}
    }
  });
});

// Tab switching
document.querySelectorAll('.tab-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
    document.querySelectorAll('.tab-panel').forEach(p => p.classList.remove('active'));
    btn.classList.add('active');
    document.getElementById('tab-' + btn.dataset.tab).classList.add('active');
  });
});

function setupBackendEvents() {
  listen('status', (event) => {
    document.getElementById('statusText').textContent = event.payload;
  });

  listen('transcript', (event) => {
    const log = document.getElementById('transcriptLog');
    log.textContent += event.payload + '\n';
    log.scrollTop = log.scrollHeight;
  });

  listen('recording', (event) => {
    const dot = document.getElementById('statusDot');
    dot.classList.toggle('recording', event.payload);
  });

  listen('models-loaded', (event) => {
    const d = event.payload;
    document.getElementById('whisperModel').textContent = d.whisper;
    document.getElementById('whisperStatus').textContent = d.whisperStatus;
    document.getElementById('refinerModel').textContent = d.refiner;
    document.getElementById('refinerStatus').textContent = d.refinerStatus;
  });
}

// Hotkey capture
const hotkeyBtn = document.getElementById('hotkeyCapture');
let hotkeyListening = false;

const KEY_NAMES = {
  'Alt': 'Option', 'Meta': 'Cmd', 'Control': 'Ctrl', 'Shift': 'Shift',
  ' ': 'Space', 'ArrowUp': 'Up', 'ArrowDown': 'Down', 'ArrowLeft': 'Left', 'ArrowRight': 'Right',
};

function friendlyKey(key) {
  return KEY_NAMES[key] || (key.length === 1 ? key.toUpperCase() : key);
}

let comboKeys = [];
let comboTimeout = null;

hotkeyBtn.addEventListener('click', () => {
  hotkeyListening = true;
  comboKeys = [];
  hotkeyBtn.classList.add('listening');
  hotkeyBtn.textContent = 'Press keys...';
});

document.addEventListener('keydown', (e) => {
  if (!hotkeyListening) return;
  e.preventDefault();
  e.stopPropagation();

  const key = friendlyKey(e.key);

  // Don't add duplicates
  if (comboKeys.includes(key)) return;
  if (comboKeys.length >= 3) return;

  comboKeys.push(key);
  hotkeyBtn.textContent = comboKeys.join(' + ');

  // Reset the auto-commit timer on each new key
  clearTimeout(comboTimeout);
  comboTimeout = setTimeout(async () => {
    if (comboKeys.length > 0) {
      const combo = comboKeys.join(' + ');
      hotkeyBtn.textContent = combo;
      hotkeyListening = false;
      hotkeyBtn.classList.remove('listening');
      document.getElementById('hotkeyDisplay').textContent = combo;
      markSettingsDirty();

      // Save hotkey immediately
      if (invoke) {
        try {
          const cfg = await invoke('get_config');
          cfg.hotkey = combo;
          await invoke('save_config', { config: cfg });
        } catch (e) {}
      }
    }
  }, 800);
}, true);

document.addEventListener('keyup', (e) => {
  if (!hotkeyListening) return;
  e.preventDefault();
  e.stopPropagation();
}, true);

// Click elsewhere to cancel
document.addEventListener('click', (e) => {
  if (hotkeyListening && e.target !== hotkeyBtn) {
    hotkeyListening = false;
    clearTimeout(comboTimeout);
    hotkeyBtn.classList.remove('listening');
    if (!comboKeys.length) hotkeyBtn.textContent = 'Option';
  }
});

// Auto-save settings on any change
let saveTimeout = null;
function autoSaveSettings() {
  clearTimeout(saveTimeout);
  saveTimeout = setTimeout(async () => {
    if (!invoke) return;
    try {
      const cfg = await invoke('get_config');
      cfg.whisper_model = document.getElementById('whisperModelInput').value;
      cfg.language = document.getElementById('languageInput').value;
      cfg.vad_threshold = parseFloat(document.getElementById('vadInput').value) || 0.003;
      cfg.max_record_seconds = parseInt(document.getElementById('maxRecordInput').value) || 30;
      cfg.refiner_mode = document.getElementById('refinerModeInput').value;
      cfg.use_refiner = cfg.refiner_mode !== 'off';
      cfg.dictation_enabled = document.getElementById('dictationToggle').checked;
      cfg.snippets = getSnippets();
      await invoke('save_config', { config: cfg });
    } catch (e) {}
  }, 500);
}

document.querySelectorAll('#tab-settings input, #tab-settings select').forEach(el => {
  el.addEventListener('input', autoSaveSettings);
  el.addEventListener('change', autoSaveSettings);
});

function markSettingsDirty() {
  autoSaveSettings();
}


// Personal Dictionary
let dictionary = [];

function renderDictionary() {
  const container = document.getElementById('dictChips');
  if (!container) return;
  if (dictionary.length === 0) {
    container.innerHTML = '<span class="dict-empty">No words yet</span>';
    return;
  }
  container.innerHTML = dictionary.map((word, i) =>
    `<span class="dict-chip">${word}<button class="dict-chip-remove" data-idx="${i}">&times;</button></span>`
  ).join('');
  container.querySelectorAll('.dict-chip-remove').forEach(btn => {
    btn.addEventListener('click', () => {
      dictionary.splice(parseInt(btn.dataset.idx), 1);
      renderDictionary();
      saveDictionary();
    });
  });
}

async function saveDictionary() {
  if (!invoke) return;
  try {
    const cfg = await invoke('get_config');
    cfg.dictionary = dictionary;
    await invoke('save_config', { config: cfg });
  } catch (e) {}
}

document.body.addEventListener('click', (e) => {
  if (e.target && (e.target.id === 'addDictBtn' || e.target.closest('#addDictBtn'))) {
    const input = document.getElementById('dictInput');
    const word = input.value.trim();
    if (word && !dictionary.includes(word)) {
      dictionary.push(word);
      renderDictionary();
      saveDictionary();
    }
    input.value = '';
    input.focus();
  }
});

// Enter key in dict input also adds
document.body.addEventListener('keydown', (e) => {
  if (e.target && e.target.id === 'dictInput' && e.key === 'Enter') {
    e.preventDefault();
    const word = e.target.value.trim();
    if (word && !dictionary.includes(word)) {
      dictionary.push(word);
      renderDictionary();
      saveDictionary();
    }
    e.target.value = '';
  }
});

// Theme switching
const themes = {
  'speakeasy': {
    '--bg': '#0a0908', '--surface': '#141210', '--surface-2': '#1c1915',
    '--border': '#2a251e', '--border-strong': '#3d3528',
    '--text': '#e8dcc4', '--text-muted': '#8a7e66',
    '--gold': '#c9a961', '--gold-bright': '#e6c989', '--gold-dim': '#7a6538',
    '--red': '#a84339',
  },
  'absinthe': {
    '--bg': '#0b0d0a', '--surface': '#141712', '--surface-2': '#1a1e17',
    '--border': '#232820', '--border-strong': '#34392d',
    '--text': '#d8dccb', '--text-muted': '#7a8070',
    '--gold': '#a8c976', '--gold-bright': '#c4dd94', '--gold-dim': '#5e7342',
    '--red': '#a84339',
  },
  'oxblood': {
    '--bg': '#0c0808', '--surface': '#14100f', '--surface-2': '#1c1614',
    '--border': '#2a201e', '--border-strong': '#3d2a27',
    '--text': '#e8d8cc', '--text-muted': '#8a7266',
    '--gold': '#c96555', '--gold-bright': '#e38876', '--gold-dim': '#7a3a30',
    '--red': '#a84339',
  },
  'ivory': {
    '--bg': '#120f0a', '--surface': '#1c1812', '--surface-2': '#24201a',
    '--border': '#332d24', '--border-strong': '#473e32',
    '--text': '#f1e7d2', '--text-muted': '#96886c',
    '--gold': '#e0c78c', '--gold-bright': '#f3dcab', '--gold-dim': '#8f7544',
    '--red': '#a84339',
  },
};

document.querySelectorAll('.theme-card').forEach(card => {
  card.addEventListener('click', async () => {
    const theme = card.dataset.theme;
    document.querySelectorAll('.theme-card').forEach(c => c.classList.remove('active'));
    card.classList.add('active');
    applyTheme(theme);
    if (invoke) {
      try {
        const cfg = await invoke('get_config');
        cfg.theme = theme;
        await invoke('save_config', { config: cfg });
      } catch (e) {}
    }
  });
});

function applyTheme(name) {
  const vars = themes[name];
  if (!vars) return;
  const root = document.documentElement;
  for (const [key, val] of Object.entries(vars)) {
    root.style.setProperty(key, val);
  }
}

// Settings nav switching
document.querySelectorAll('.settings-nav-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('.settings-nav-btn').forEach(b => b.classList.remove('active'));
    document.querySelectorAll('.settings-section').forEach(s => s.classList.remove('active'));
    btn.classList.add('active');
    document.getElementById('section-' + btn.dataset.section).classList.add('active');
  });
});

// -- Load initial config --

async function loadConfig() {
  try {
    const cfg = await invoke('get_config');
    document.getElementById('refinerModelInput').value = cfg.refiner_model;
    document.getElementById('languageInput').value = cfg.language;
    document.getElementById('vadInput').value = cfg.vad_threshold;
    document.getElementById('maxRecordInput').value = cfg.max_record_seconds;
    document.getElementById('refinerModeInput').value = cfg.refiner_mode || (cfg.use_refiner ? 'rewrite' : 'off');
    document.getElementById('dictationToggle').checked = cfg.dictation_enabled !== false;

    // Set hotkey display
    if (cfg.hotkey) {
      document.getElementById('hotkeyCapture').textContent = cfg.hotkey;
      document.getElementById('hotkeyDisplay').textContent = cfg.hotkey;
    }

    // Set theme
    const theme = cfg.theme || 'speakeasy';
    applyTheme(theme);
    document.querySelectorAll('.theme-card').forEach(c => {
      c.classList.toggle('active', c.dataset.theme === theme);
    });

    // Set tone
    const tone = cfg.tone || 'normal';
    document.querySelectorAll('.tone-btn').forEach(b => {
      b.classList.toggle('active', b.dataset.tone === tone);
    });
    document.getElementById('toneDesc').textContent = toneDescs[tone] || toneDescs.normal;
    updateRefinerVisibility();

    // Select whisper model
    const sel = document.getElementById('whisperModelInput');
    for (let opt of sel.options) {
      if (opt.value === cfg.whisper_model) opt.selected = true;
    }

    // Load snippets
    if (cfg.snippets) {
      cfg.snippets.forEach(s => addSnippetRow(s.trigger, s.replacement));
    }

    // Load dictionary
    dictionary = cfg.dictionary || [];
    renderDictionary();

    // Load stats
    document.getElementById('wordsToday').textContent = cfg.stats?.words_today || 0;
    document.getElementById('transcriptionsToday').textContent = cfg.stats?.transcriptions_today || 0;

    // Update snippet count
    updateSnippetCount();
  } catch (e) {
    console.error('Failed to load config:', e);
  }
}


// -- Snippets --

const snippetList = document.getElementById('snippetList');

// Use event delegation on document body for the add button
document.body.addEventListener('click', (e) => {
  if (e.target && (e.target.id === 'addSnippetBtn' || e.target.closest('#addSnippetBtn'))) {
      addSnippetRow('', '');
    updateSnippetCount();
  }
});

function addSnippetRow(trigger, replacement) {
  // Hide empty state
  const empty = document.getElementById('snippetsEmpty');
  if (empty) empty.style.display = 'none';

  const card = document.createElement('div');
  card.className = 'snippet-card';
  card.innerHTML = `
    <div class="snippet-fields">
      <div class="snippet-field">
        <label>When you say</label>
        <input type="text" placeholder="e.g. my email address" value="${trigger || ''}">
      </div>
      <span class="snippet-arrow">&rarr;</span>
      <div class="snippet-field">
        <label>Type this instead</label>
        <textarea placeholder="e.g. john@example.com" rows="1">${replacement || ''}</textarea>
      </div>
    </div>
    <button class="remove-btn" title="Remove">&times;</button>
  `;
  card.querySelector('.remove-btn').addEventListener('click', () => {
    card.remove();
    updateSnippetCount();
    // Show empty state if no cards left
    if (snippetList.querySelectorAll('.snippet-card').length === 0) {
      const empty = document.getElementById('snippetsEmpty');
      if (empty) empty.style.display = 'flex';
    }
  });
  card.querySelectorAll('input, textarea').forEach(el => {
    el.addEventListener('change', saveSnippets);
  });
  // Auto-grow textarea
  const ta = card.querySelector('textarea');
  if (ta) {
    const autoGrow = () => { ta.style.height = 'auto'; ta.style.height = ta.scrollHeight + 'px'; };
    ta.addEventListener('input', autoGrow);
    setTimeout(autoGrow, 0);
  }
  snippetList.appendChild(card);
}

function saveSnippets() {
  // Auto-save snippets when edited
  if (!invoke) return;
  const snippets = getSnippets();
  invoke('get_config').then(cfg => {
    cfg.snippets = snippets;
    invoke('save_config', { config: cfg });
  }).catch(() => {});
}

function getSnippets() {
  const snippets = [];
  snippetList.querySelectorAll('.snippet-card').forEach(card => {
    const trigger = card.querySelector('input').value.trim();
    const replacement = card.querySelector('textarea').value;
    if (trigger || replacement) {
      snippets.push({ trigger, replacement });
    }
  });
  return snippets;
}

function updateSnippetCount() {
  const count = snippetList.querySelectorAll('.snippet-card').length;
  document.getElementById('snippetCount').textContent =
    `${count} voice shortcut${count === 1 ? '' : 's'} configured`;
}

// -- Stats + model status polling --

async function refreshStats() {
  try {
    const [words, transcriptions] = await invoke('get_stats');
    document.getElementById('wordsToday').textContent = words;
    document.getElementById('transcriptionsToday').textContent = transcriptions;
  } catch (e) {}
}

async function refreshModelStatus() {
  try {
    const d = await invoke('get_model_status');
    document.getElementById('whisperModel').textContent = d.whisper;
    document.getElementById('whisperStatus').textContent = d.whisperStatus;
    document.getElementById('refinerModel').textContent = d.refiner;
    document.getElementById('refinerStatus').textContent = d.refinerStatus;
  } catch (e) {}
}

setInterval(refreshStats, 5000);
setInterval(refreshModelStatus, 2000);

// Update status box from model status (catches missed early events)
setInterval(async () => {
  if (!invoke) return;
  try {
    const d = await invoke('get_model_status');
    const statusEl = document.getElementById('statusText');
    if (statusEl && statusEl.textContent === 'Starting...') {
      if (d.whisperStatus === 'Loaded') {
        statusEl.textContent = 'Ready';
      } else {
        statusEl.textContent = 'Loading models...';
      }
    }
  } catch (e) {}
}, 1000);

// -- Init --

initTauri();
