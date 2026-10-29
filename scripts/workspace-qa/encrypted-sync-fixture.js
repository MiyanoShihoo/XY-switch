// Renderer-only fixture. Rust crypto, file I/O and transactions are covered by backend tests.
// No real filesystem, credential-store or network operations. Never retain passphrases in QA logs.
(() => {
  const qa = window.__XY_QA__, clone = value => structuredClone(value), now = () => Math.floor(Date.now() / 1000);
  const os = localStorage.getItem('xy-sync-qa:os') || 'windows';
  qa.platform.os = os; qa.platform.mobile = os === 'android' || os === 'ios';
  qa.proxy.running = localStorage.getItem('xy-sync-qa:running') === 'true';
  const favorite = { providerId: 'codex', sessionId: 'qa-session' };
  const provider = { id: '10000000-0000-4000-8000-000000000001', name: 'QA encrypted provider', providerType: 'openai_compatible', appGroup: 'Codex', endpoint: 'https://example.invalid/v1', model: 'qa-alpha', upstreamFormat: 'openai_chat', isFullUrl: false, modelMappings: [] };
  const price = { id: 'qa-alpha', inputPerMillion: 1, outputPerMillion: 2, cacheReadPerMillion: null, cacheCreationPerMillion: null };
  const s = qa.sync = { favorites: [], migrated: false, sequence: 0, tickets: {}, backups: [], ttl: 300, large: false, cancelImport: false, cancelSave: false, auditRecorded: true };
  const payload = () => ({ schema: 1, providers: [provider], favorites: clone(s.favorites), prices: [price] });
  function preview(action, content) {
    const ticketId = 'qa-sync-' + (++s.sequence), expiresAt = now() + s.ttl;
    s.tickets[ticketId] = { action, content: clone(content), expiresAt };
    return { ticketId, expiresAt, content };
  }
  function take(id, action, actual, expected) {
    if (actual !== expected) throw Error('Missing explicit confirmation');
    const ticket = s.tickets[id]; delete s.tickets[id];
    if (!ticket || ticket.action !== action || ticket.expiresAt <= now()) throw Error('Expired or mismatched preview');
    return ticket;
  }
  function changes() {
    const fresh = { ...provider, id: '10000000-0000-4000-8000-000000000002', name: 'New disabled provider' };
    const incomingFavorite = { providerId: 'codex', sessionId: 'qa-imported' };
    const rows = [
      { key: 'provider:' + provider.id, kind: 'provider', status: 'conflict', effect: 'keep_old_target_create_disabled_copy', local: provider, incoming: { ...provider, endpoint: 'https://changed.example.invalid/v1' } },
      { key: 'provider:' + fresh.id, kind: 'provider', status: 'add', effect: 'new_disabled_provider', local: null, incoming: fresh },
      { key: 'price:qa-alpha', kind: 'price', status: 'conflict', effect: 'custom_price_only_history_unchanged', local: { pricing: price, source: 'custom', updatedAt: 1 }, incoming: { ...price, inputPerMillion: 4 } },
      { key: 'favorite:' + JSON.stringify(['codex', 'qa-imported']), kind: 'favorite', status: 'add', effect: 'identifier_only_no_session_content', local: null, incoming: incomingFavorite },
      { key: 'favorite:' + JSON.stringify(['codex', 'qa-session']), kind: 'favorite', status: 'unchanged', effect: 'identifier_only_no_session_content', local: favorite, incoming: favorite },
    ];
    if (s.large) for (let n = 0; n < 52; n++) rows.push({ key: 'price:page-' + n, kind: 'price', status: 'add', effect: 'custom_price_only_history_unchanged', local: null, incoming: { ...price, id: 'page-' + n } });
    return rows;
  }
  const handlers = {
    load_session_favorites: ({ legacy, legacyAvailable }) => {
      if (!s.migrated) {
        if (!legacyAvailable) throw Error('Legacy favorites unavailable');
        if (legacy) { const parsed = JSON.parse(legacy); if (parsed.version !== 1) throw Error('Invalid legacy favorites data'); s.favorites = parsed.keys.map(k => { const [providerId, sessionId] = JSON.parse(k); return { providerId, sessionId }; }); }
        s.migrated = true;
      }
      return s.favorites;
    },
    set_session_favorite: ({ favorite, enabled }) => {
      s.favorites = s.favorites.filter(f => f.providerId !== favorite.providerId || f.sessionId !== favorite.sessionId);
      if (enabled) s.favorites.push(clone(favorite));
    },
    preview_sync_export: () => preview('export', payload()),
    save_sync_export: ({ ticketId, passphrase, confirmation }) => {
      take(ticketId, 'export', confirmation, 'EXPORT');
      if ([...passphrase].length < 12) throw Error('Short passphrase');
      return { saved: !s.cancelSave, auditRecorded: s.auditRecorded };
    },
    preview_sync_import: ({ passphrase }) => {
      if (passphrase !== 'fixture long sync passphrase') throw Error('Wrong passphrase or damaged encrypted file');
      return s.cancelImport ? null : preview('import', changes());
    },
    apply_sync_import: ({ ticketId, choices, confirmation }) => {
      if (qa.proxy.running) throw Error('Stop proxy and restore takeovers before importing');
      const { content } = take(ticketId, 'import', confirmation, 'IMPORT');
      const changed = content.filter(r => r.status !== 'unchanged');
      if (choices.length !== changed.length || changed.some(r => choices.filter(c => c.key === r.key).length !== 1)) throw Error('Choose every changed item');
      const selected = choices.filter(c => c.useIncoming), backup = { id: 'qa-backup-' + (++s.sequence), createdAt: now(), used: false, favorites: clone(s.favorites) };
      if (!selected.length) throw Error('No incoming changes selected');
      for (const choice of selected) { const row = changed.find(r => r.key === choice.key); if (row.kind === 'favorite') s.favorites.push(clone(row.incoming)); }
      s.backups.unshift(backup);
      return { backupId: backup.id, applied: selected.length, disabledCopies: selected.filter(c => c.key.startsWith('provider:')).length };
    },
    list_sync_backups: () => s.backups.map(({ id, createdAt, used }) => ({ id, createdAt, used })),
    preview_sync_rollback: ({ id }) => { const backup = s.backups.find(b => b.id === id); return preview('rollback', { before: payload(), after: { configuration: { ...payload(), favorites: backup.favorites }, retainedDisabledProviders: [provider] } }); },
    apply_sync_rollback: ({ ticketId, confirmation }) => { take(ticketId, 'rollback', confirmation, 'ROLLBACK SYNC'); const backup = s.backups.find(b => !b.used); s.favorites = clone(backup.favorites); backup.used = true; },
    cancel_sync_preview: ({ ticketId }) => { delete s.tickets[ticketId]; },
  };
  const original = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async (command, args = {}) => {
    if (!Object.hasOwn(handlers, command)) return original(command, args);
    const logged = clone(args); if (Object.hasOwn(logged, 'passphrase')) logged.passphrase = '[redacted]';
    qa.calls.push({ command, args: logged });
    if (qa.hold.includes(command)) await new Promise(resolve => qa.pending.push({ command, resolve }));
    if (qa.fail[command]) throw Error(qa.fail[command]);
    const result = handlers[command](args);
    return result === undefined ? null : clone(result);
  };
})();
