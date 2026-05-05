<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  type McpInfo = {
    url: string;
    token: string;
    bound_port: number;
    configured_port: number;
  };

  let {
    onclose,
    showToast,
  }: {
    onclose: () => void;
    showToast: (kind: 'ok' | 'err' | 'info', text: string) => void;
  } = $props();

  let mcp = $state<McpInfo | null>(null);
  let portInput = $state('');
  let registryUrl = $state('');

  onMount(async () => {
    try {
      mcp = await invoke<McpInfo | null>('get_mcp_status');
      if (mcp) portInput = mcp.configured_port.toString();
    } catch {}
    try {
      registryUrl = await invoke<string>('get_registry_url');
    } catch {}
  });

  async function copyUrl() {
    if (!mcp) return;
    await navigator.clipboard.writeText(mcp.url);
    showToast('ok', 'URL copied.');
  }
  async function copyToken() {
    if (!mcp) return;
    await navigator.clipboard.writeText(mcp.token);
    showToast('ok', 'Token copied.');
  }
  async function applyPort() {
    const p = parseInt(portInput, 10);
    if (Number.isNaN(p) || p < 1024 || p > 65535) {
      showToast('err', 'Port must be between 1024 and 65535.');
      return;
    }
    try {
      await invoke('set_mcp_port', { port: p });
      mcp = await invoke<McpInfo | null>('get_mcp_status');
      showToast('ok', `MCP server now on port ${p}. Restart your AI apps.`);
    } catch (e) {
      showToast('err', `Error: ${e}`);
    }
  }
  async function rotateToken() {
    if (!confirm('Rotate the MCP token? AI app registrations will be re-pushed.')) return;
    try {
      await invoke('rotate_mcp_token');
      mcp = await invoke<McpInfo | null>('get_mcp_status');
      showToast('ok', 'Token rotated. Restart your AI apps.');
    } catch (e) {
      showToast('err', `Error: ${e}`);
    }
  }
  async function saveRegistry() {
    if (!registryUrl.trim()) {
      showToast('err', 'Registry URL cannot be empty.');
      return;
    }
    try {
      await invoke('set_registry_url', { url: registryUrl.trim() });
      showToast('ok', 'Registry URL saved.');
    } catch (e) {
      showToast('err', `Error: ${e}`);
    }
  }
</script>

<div class="overlay" onclick={onclose} onkeydown={() => {}} role="presentation">
  <div class="panel" onclick={(e) => e.stopPropagation()} onkeydown={() => {}} role="dialog">
    <header class="head">
      <h2>Advanced</h2>
      <button class="close" onclick={onclose} aria-label="Close">×</button>
    </header>

    <div class="body">
      <!-- ── MCP server ── -->
      <section class="block">
        <h3>MCP server</h3>
        <p class="desc">
          AutoPipe registers Claude Desktop, Codex CLI, and Gemini CLI
          automatically when you click <strong>Save and Register</strong>.
          Use the values below to connect a different MCP-compatible
          tool manually.
        </p>
        {#if mcp}
          <div class="rows">
            <div class="row">
              <span class="label">URL</span>
              <code>{mcp.url}</code>
              <button class="ghost" onclick={copyUrl}>Copy</button>
            </div>
            <div class="row">
              <span class="label">Port</span>
              <input type="number" bind:value={portInput} />
              <button class="ghost" onclick={applyPort}>Apply</button>
            </div>
            <div class="row">
              <span class="label">Token</span>
              <span class="masked">●●●●●●●●●●●●</span>
              <button class="ghost" onclick={copyToken}>Copy</button>
              <button class="ghost" onclick={rotateToken}>Rotate</button>
            </div>
          </div>
        {:else}
          <p class="loading">MCP server is starting…</p>
        {/if}
      </section>

      <!-- ── Pipeline registry ── -->
      <section class="block">
        <h3>Pipeline registry</h3>
        <p class="desc">
          AutoPipe pulls pipelines from
          <a href="https://hub.autopipe.org" target="_blank" rel="noopener">
            hub.autopipe.org</a>
          by default. Set another URL here if your team or institution
          hosts its own AutoPipeHub instance.
        </p>
        <div class="row">
          <span class="label">URL</span>
          <input type="text" bind:value={registryUrl} placeholder="https://hub.autopipe.org" />
          <button class="ghost" onclick={saveRegistry}>Save</button>
        </div>
      </section>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(15, 23, 42, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
    padding: 32px;
  }
  .panel {
    background: var(--bg-card);
    border-radius: 12px;
    width: 100%;
    max-width: 620px;
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 16px 40px rgba(15, 23, 42, 0.2);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 22px;
    border-bottom: 1px solid var(--border);
  }
  .head h2 {
    margin: 0;
    font-size: 1.1rem;
    font-weight: 600;
    color: var(--text);
  }
  .close {
    width: 28px;
    height: 28px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 1.4rem;
    cursor: pointer;
    border-radius: 4px;
  }
  .close:hover {
    background: var(--bg-soft);
    color: var(--text);
  }

  .body {
    padding: 18px 22px 22px;
    overflow-y: auto;
  }
  .block { margin-bottom: 24px; padding-bottom: 24px; border-bottom: 1px solid var(--border); }
  .block:last-child { margin-bottom: 0; padding-bottom: 0; border-bottom: none; }
  .block h3 {
    margin: 0 0 6px;
    font-size: 0.95rem;
    font-weight: 600;
    color: var(--text);
  }
  .desc {
    margin: 0 0 12px;
    font-size: 0.83rem;
    color: var(--text-muted);
    line-height: 1.5;
  }
  .desc a {
    color: var(--accent);
    text-decoration: underline;
  }

  .rows { display: flex; flex-direction: column; gap: 8px; }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .label {
    width: 60px;
    font-size: 0.8rem;
    color: var(--text-muted);
    font-weight: 500;
  }
  code {
    flex: 1;
    font-family: 'SF Mono', Menlo, monospace;
    font-size: 0.8rem;
    color: var(--text);
    background: var(--bg-soft);
    padding: 6px 10px;
    border-radius: 4px;
    overflow-x: auto;
    white-space: nowrap;
  }
  .masked {
    flex: 1;
    font-family: monospace;
    color: var(--text-faint);
    padding: 6px 10px;
  }
  input {
    flex: 1;
    padding: 6px 10px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    font-size: 0.85rem;
    color: var(--text);
    font-family: inherit;
    background: var(--bg-card);
  }
  input[type="number"] { flex: 0 0 90px; }
  input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-light);
  }
  .ghost {
    padding: 6px 12px;
    background: transparent;
    color: var(--accent);
    border: 1px solid var(--accent);
    border-radius: 4px;
    font-size: 0.78rem;
    font-weight: 500;
    cursor: pointer;
  }
  .ghost:hover { background: var(--accent-light); }
  .loading {
    color: var(--text-faint);
    font-size: 0.85rem;
    margin: 0;
  }
</style>
