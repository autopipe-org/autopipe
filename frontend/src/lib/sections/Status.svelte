<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  type McpInfo = {
    url: string;
    token: string;
    bound_port: number;
    configured_port: number;
  };

  let { showToast }: { showToast: (kind: 'ok' | 'err' | 'info', text: string) => void } = $props();

  let info = $state<McpInfo | null>(null);
  let portInput = $state('');

  onMount(async () => {
    await refresh();
  });

  async function refresh() {
    try {
      info = await invoke<McpInfo | null>('get_mcp_status');
      if (info) portInput = info.configured_port.toString();
    } catch {}
  }

  async function copyToken() {
    if (!info) return;
    await navigator.clipboard.writeText(info.token);
    showToast('ok', 'Token copied to clipboard.');
  }

  async function applyPort() {
    const p = parseInt(portInput, 10);
    if (Number.isNaN(p) || p < 1024 || p > 65535) {
      showToast('err', 'Port must be between 1024 and 65535.');
      return;
    }
    try {
      await invoke('set_mcp_port', { port: p });
      await refresh();
      showToast('ok', `MCP server now on port ${p}. Restart your AI apps.`);
    } catch (e) {
      showToast('err', `Error: ${e}`);
    }
  }

  async function rotateToken() {
    if (!confirm('Rotate the MCP token? Existing AI app registrations will be re-pushed automatically.')) return;
    try {
      await invoke('rotate_mcp_token');
      await refresh();
      showToast('ok', 'Token rotated. Restart your AI apps.');
    } catch (e) {
      showToast('err', `Error: ${e}`);
    }
  }
</script>

{#if info}
  <div class="rows">
    <div class="row">
      <span class="label">URL</span>
      <code>{info.url}</code>
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

<style>
  .rows {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .label {
    width: 60px;
    font-size: 0.82rem;
    color: #64748b;
    font-weight: 500;
  }
  code {
    flex: 1;
    font-family: 'SF Mono', Menlo, monospace;
    font-size: 0.82rem;
    color: #0f172a;
    background: #f1f5f9;
    padding: 6px 10px;
    border-radius: 4px;
  }
  .masked {
    flex: 1;
    font-family: monospace;
    color: #94a3b8;
    padding: 6px 10px;
  }
  input {
    padding: 6px 10px;
    border: 1px solid #cbd5e1;
    border-radius: 4px;
    width: 90px;
    font-size: 0.85rem;
  }
  input:focus {
    outline: none;
    border-color: #0f4c5c;
  }
  .ghost {
    padding: 6px 12px;
    background: transparent;
    color: #0f4c5c;
    border: 1px solid #cbd5e1;
    border-radius: 4px;
    font-size: 0.8rem;
    font-weight: 500;
    cursor: pointer;
  }
  .ghost:hover {
    background: #f1f5f9;
    border-color: #94a3b8;
  }
  .loading {
    color: #94a3b8;
    font-size: 0.9rem;
    margin: 0;
  }
</style>
