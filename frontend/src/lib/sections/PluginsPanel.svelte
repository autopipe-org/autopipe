<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  type InstalledPlugin = {
    name: string;
    version: string;
    description: string;
    author: string;
    extensions: string[];
  };
  type RegistryPlugin = {
    name: string;
    version: string;
    description: string;
    author: string;
    github_url: string;
  };
  type Row = {
    name: string;
    description: string;
    installed_version: string | null; // null = not installed
    latest_version: string | null;    // null = not in registry
    extensions: string[];
    author: string;
    busy?: 'install' | 'update' | 'uninstall';
    error?: string;
  };

  let { onclose, showToast } = $props<{
    onclose: () => void;
    showToast: (kind: 'ok' | 'err' | 'info', text: string) => void;
  }>();

  let rows = $state<Row[]>([]);
  let loading = $state(true);
  let search = $state('');

  // Compare semantic versions naively (string-segment numeric). Returns
  //   -1 if a < b, 0 if equal, 1 if a > b. Anything unparseable is treated
  //   as 0 so the user still sees something.
  function cmpVersion(a: string, b: string): number {
    const pa = a.split('.').map((s) => parseInt(s, 10));
    const pb = b.split('.').map((s) => parseInt(s, 10));
    const len = Math.max(pa.length, pb.length);
    for (let i = 0; i < len; i++) {
      const x = pa[i] ?? 0;
      const y = pb[i] ?? 0;
      if (Number.isNaN(x) || Number.isNaN(y)) return 0;
      if (x < y) return -1;
      if (x > y) return 1;
    }
    return 0;
  }

  async function refresh() {
    loading = true;
    try {
      const [installed, registry] = await Promise.all([
        invoke<InstalledPlugin[]>('list_installed_plugins'),
        invoke<RegistryPlugin[]>('list_registry_plugins').catch((e) => {
          showToast('err', `Couldn't reach the plugin registry: ${e}`);
          return [] as RegistryPlugin[];
        }),
      ]);

      const map = new Map<string, Row>();
      for (const r of registry) {
        map.set(r.name, {
          name: r.name,
          description: r.description,
          installed_version: null,
          latest_version: r.version || null,
          extensions: [],
          author: r.author,
        });
      }
      for (const p of installed) {
        const existing = map.get(p.name);
        if (existing) {
          existing.installed_version = p.version;
          existing.extensions = p.extensions;
          if (!existing.description) existing.description = p.description;
          if (!existing.author) existing.author = p.author;
        } else {
          // Locally installed but not in registry — show as installed-only.
          map.set(p.name, {
            name: p.name,
            description: p.description,
            installed_version: p.version,
            latest_version: null,
            extensions: p.extensions,
            author: p.author,
          });
        }
      }
      rows = [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
    } finally {
      loading = false;
    }
  }

  onMount(refresh);

  function updateAvailable(r: Row): boolean {
    if (!r.installed_version || !r.latest_version) return false;
    return cmpVersion(r.installed_version, r.latest_version) < 0;
  }

  async function doInstall(r: Row) {
    r.busy = 'install';
    rows = rows;
    try {
      const res = await invoke<{ version: string }>('install_plugin', {
        pluginName: r.name,
      });
      r.installed_version = res.version;
      showToast('ok', `Installed ${r.name} v${res.version}.`);
    } catch (e) {
      r.error = String(e);
      showToast('err', `Install failed: ${e}`);
    } finally {
      r.busy = undefined;
      rows = rows;
    }
  }

  async function doUpdate(r: Row) {
    r.busy = 'update';
    rows = rows;
    try {
      const res = await invoke<{ version: string }>('update_plugin', {
        pluginName: r.name,
      });
      r.installed_version = res.version;
      showToast('ok', `Updated ${r.name} to v${res.version}.`);
    } catch (e) {
      r.error = String(e);
      showToast('err', `Update failed: ${e}`);
    } finally {
      r.busy = undefined;
      rows = rows;
    }
  }

  async function doUninstall(r: Row) {
    r.busy = 'uninstall';
    rows = rows;
    try {
      await invoke('uninstall_plugin', { pluginName: r.name });
      r.installed_version = null;
      showToast('ok', `Uninstalled ${r.name}.`);
      // If the plugin was installed-only (not in registry), drop the row
      // entirely so it isn't a dead entry the user can't reinstall.
      if (!r.latest_version) {
        rows = rows.filter((x) => x.name !== r.name);
      }
    } catch (e) {
      r.error = String(e);
      showToast('err', `Uninstall failed: ${e}`);
    } finally {
      r.busy = undefined;
      rows = rows;
    }
  }

  const filtered = $derived(() => {
    const q = search.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter(
      (r) =>
        r.name.toLowerCase().includes(q) ||
        r.description.toLowerCase().includes(q) ||
        r.extensions.some((e) => e.toLowerCase().includes(q))
    );
  });
</script>

<div class="overlay" role="dialog" aria-modal="true" aria-labelledby="plugins-title">
  <div class="modal">
    <header class="modal-head">
      <h2 id="plugins-title">Plugins</h2>
      <button class="icon-btn" onclick={onclose} aria-label="Close">×</button>
    </header>

    <div class="modal-body">
      <p class="hint">
        Plugins add viewer support for additional file formats in the Results
        Viewer. Plugins run JavaScript in the browser — only install plugins
        from authors you trust.
      </p>

      <input
        type="search"
        class="search"
        placeholder="Search plugins…"
        bind:value={search}
      />

      {#if loading}
        <p class="empty">Loading plugins…</p>
      {:else if filtered().length === 0}
        <p class="empty">
          {search ? 'No plugins match your search.' : 'No plugins available yet.'}
        </p>
      {:else}
        <ul class="plugin-list">
          {#each filtered() as r (r.name)}
            <li class="plugin-card">
              <div class="plugin-main">
                <div class="row-title">
                  <span class="name">{r.name}</span>
                  {#if r.installed_version}
                    <span class="ver">v{r.installed_version}</span>
                    {#if updateAvailable(r)}
                      <span class="badge update">update {r.latest_version}</span>
                    {/if}
                  {:else if r.latest_version}
                    <span class="ver muted">v{r.latest_version}</span>
                  {/if}
                </div>
                {#if r.description}
                  <p class="desc">{r.description}</p>
                {/if}
                {#if r.extensions.length > 0}
                  <p class="exts">
                    {r.extensions.map((e) => `.${e}`).join(', ')}
                  </p>
                {/if}
              </div>

              <div class="plugin-actions">
                {#if !r.installed_version && r.latest_version}
                  <!-- Not installed → Install only -->
                  <button
                    class="btn-primary small"
                    disabled={!!r.busy}
                    onclick={() => doInstall(r)}
                  >
                    {r.busy === 'install' ? 'Installing…' : 'Install'}
                  </button>
                {:else if r.installed_version}
                  <!-- Installed: Update (if needed) sits LEFT of Uninstall -->
                  {#if updateAvailable(r)}
                    <button
                      class="btn-outline small"
                      disabled={!!r.busy}
                      onclick={() => doUpdate(r)}
                    >
                      {r.busy === 'update' ? 'Updating…' : 'Update'}
                    </button>
                  {/if}
                  <button
                    class="btn-ghost small danger"
                    disabled={!!r.busy}
                    onclick={() => doUninstall(r)}
                  >
                    {r.busy === 'uninstall' ? 'Removing…' : 'Uninstall'}
                  </button>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <footer class="modal-foot">
      <button class="btn-ghost" onclick={refresh} disabled={loading}>
        Refresh
      </button>
      <a
        class="link"
        href="https://autopipe.org/plugins/guide"
        target="_blank"
        rel="noopener"
      >
        Plugin development guide ↗
      </a>
    </footer>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
    padding: 24px;
  }
  .modal {
    background: var(--bg-card);
    border-radius: 10px;
    border: 1px solid var(--border);
    width: 100%;
    max-width: 640px;
    max-height: 90vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.25);
  }
  .modal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 20px;
    border-bottom: 1px solid var(--border);
  }
  .modal-head h2 {
    margin: 0;
    font-size: 1.05rem;
    font-weight: 600;
  }
  .modal-body {
    flex: 1;
    overflow-y: auto;
    padding: 16px 20px;
  }
  .hint {
    margin: 0 0 12px;
    font-size: 0.82rem;
    color: var(--text-muted);
    line-height: 1.5;
  }
  .search {
    width: 100%;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 0.9rem;
    margin-bottom: 14px;
    background: var(--bg-soft);
    color: var(--text);
  }
  .search:focus {
    outline: none;
    border-color: var(--accent);
  }
  .empty {
    text-align: center;
    color: var(--text-muted);
    padding: 24px 0;
    font-size: 0.88rem;
  }
  .plugin-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .plugin-card {
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 10px 12px;
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }
  .plugin-main {
    flex: 1;
    min-width: 0;
  }
  .row-title {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .name {
    font-weight: 600;
    font-size: 0.92rem;
  }
  .ver {
    font-size: 0.78rem;
    color: var(--text-muted);
    font-family: 'SF Mono', Menlo, monospace;
  }
  .ver.muted {
    opacity: 0.6;
  }
  .badge.update {
    font-size: 0.7rem;
    color: var(--accent);
    border: 1px solid var(--accent);
    padding: 1px 6px;
    border-radius: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .desc {
    margin: 4px 0 0;
    font-size: 0.82rem;
    color: var(--text-muted);
    line-height: 1.4;
  }
  .exts {
    margin: 4px 0 0;
    font-size: 0.74rem;
    color: var(--text-faint);
    font-family: 'SF Mono', Menlo, monospace;
  }
  .plugin-actions {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }
  .btn-primary {
    background: var(--accent);
    color: #fff;
    border: 1px solid var(--accent);
    border-radius: 4px;
    padding: 5px 12px;
    font-size: 0.82rem;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-primary:hover {
    background: var(--accent-hover);
  }
  .btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .btn-outline {
    background: transparent;
    color: var(--accent);
    border: 1px solid var(--accent);
    border-radius: 4px;
    padding: 5px 12px;
    font-size: 0.82rem;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-outline:hover {
    background: var(--accent-light);
  }
  .btn-outline:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .btn-ghost {
    background: transparent;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 5px 12px;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .btn-ghost:hover {
    color: var(--text);
    border-color: var(--border-strong);
  }
  .btn-ghost.danger:hover {
    color: var(--danger);
    border-color: var(--danger);
  }
  .btn-ghost:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .modal-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 20px;
    border-top: 1px solid var(--border);
    gap: 12px;
  }
  .link {
    color: var(--accent);
    text-decoration: none;
    font-size: 0.82rem;
  }
  .link:hover {
    text-decoration: underline;
  }
  .icon-btn {
    background: transparent;
    border: none;
    font-size: 1.4rem;
    line-height: 1;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0 4px;
  }
  .icon-btn:hover {
    color: var(--text);
  }
</style>
