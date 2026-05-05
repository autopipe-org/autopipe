<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  type McpInfo = {
    url: string;
    token: string;
    bound_port: number;
    configured_port: number;
  };

  let info = $state<McpInfo | null>(null);
  let portInput = $state('');
  let message = $state('');

  onMount(async () => {
    await refresh();
  });

  async function refresh() {
    try {
      info = await invoke<McpInfo | null>('get_mcp_status');
      if (info) portInput = info.configured_port.toString();
    } catch (e) {
      message = `Failed to load status: ${e}`;
    }
  }

  async function copyToken() {
    if (!info) return;
    await navigator.clipboard.writeText(info.token);
    message = 'Token copied to clipboard.';
  }

  async function applyPort() {
    const p = parseInt(portInput, 10);
    if (Number.isNaN(p) || p < 1024 || p > 65535) {
      message = 'Port must be between 1024 and 65535.';
      return;
    }
    try {
      await invoke('set_mcp_port', { port: p });
      await refresh();
      message = `MCP server now on port ${p}. Restart your AI apps.`;
    } catch (e) {
      message = `Error: ${e}`;
    }
  }

  async function rotateToken() {
    if (!confirm('Rotate token? Existing AI app registrations will be re-pushed automatically.')) return;
    try {
      await invoke('rotate_mcp_token');
      await refresh();
      message = 'Token rotated and re-registered. Restart your AI apps.';
    } catch (e) {
      message = `Error: ${e}`;
    }
  }
</script>

<section>
  <h2>Status</h2>

  {#if info}
    <div class="info">
      <div class="row">
        <span class="label">URL</span>
        <code>{info.url}</code>
      </div>
      <div class="row">
        <span class="label">Port</span>
        <input type="number" bind:value={portInput} />
        <button class="secondary" onclick={applyPort}>Apply</button>
      </div>
      <div class="row">
        <span class="label">Token</span>
        <span class="masked">●●●●●●●●●●●●</span>
        <button class="secondary" onclick={copyToken}>Copy</button>
        <button class="secondary" onclick={rotateToken}>Rotate</button>
      </div>
    </div>
  {:else}
    <p>MCP server is starting...</p>
  {/if}

  {#if message}<p class="msg">{message}</p>{/if}
</section>

<style>
  h2 { margin: 0 0 16px; font-size: 1.25rem; }
  .info {
    background: #fff;
    border: 1px solid #e5e7eb;
    border-radius: 8px;
    padding: 16px 20px;
    max-width: 700px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 8px 0;
  }
  .label {
    width: 80px;
    font-size: 0.85rem;
    color: #6b7280;
    font-weight: 500;
  }
  code {
    font-family: 'SF Mono', monospace;
    font-size: 0.85rem;
    color: #1f2937;
    flex: 1;
  }
  .masked {
    font-family: monospace;
    color: #6b7280;
    flex: 1;
  }
  input {
    padding: 6px 10px;
    border: 1px solid #d1d5db;
    border-radius: 4px;
    width: 100px;
    font-size: 0.85rem;
  }
  button.secondary {
    padding: 6px 12px;
    background: #e5e7eb;
    color: #1f2937;
    border: none;
    border-radius: 4px;
    font-size: 0.8rem;
    cursor: pointer;
  }
  button.secondary:hover { background: #d1d5db; }
  .msg { margin-top: 16px; color: #047857; font-size: 0.9rem; }
</style>
