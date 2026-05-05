<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  let busy = $state(false);
  let message = $state('');

  async function registerAll() {
    busy = true;
    message = '';
    try {
      const result = await invoke<string[]>('register_mcp');
      message = `Registered in: ${result.join(', ')}\nRestart your AI app to load AutoPipe tools.`;
    } catch (e) {
      message = `Error: ${e}`;
    } finally {
      busy = false;
    }
  }

  async function unregisterAll() {
    busy = true;
    message = '';
    try {
      await invoke('unregister_mcp');
      message = 'Unregistered from all clients.';
    } catch (e) {
      message = `Error: ${e}`;
    } finally {
      busy = false;
    }
  }
</script>

<section>
  <h2>Setup</h2>
  <p>
    Register the AutoPipe MCP server in supported AI applications (Claude
    Desktop, Codex CLI, etc.). Restart your AI app afterwards to load the
    AutoPipe tools.
  </p>

  <div class="actions">
    <button class="primary" disabled={busy} onclick={registerAll}>
      Save and Register
    </button>
    <button class="secondary" disabled={busy} onclick={unregisterAll}>
      Unregister
    </button>
  </div>

  {#if message}
    <pre class="msg">{message}</pre>
  {/if}
</section>

<style>
  h2 {
    margin: 0 0 12px;
    font-size: 1.25rem;
  }
  p {
    color: #6b7280;
    margin: 0 0 20px;
  }
  .actions {
    display: flex;
    gap: 12px;
  }
  button {
    padding: 10px 18px;
    border-radius: 6px;
    font-size: 0.9rem;
    font-weight: 500;
    border: none;
    cursor: pointer;
  }
  .primary {
    background: #0f4c5c;
    color: #fff;
  }
  .primary:hover {
    background: #0d3d4a;
  }
  .secondary {
    background: #e5e7eb;
    color: #1f2937;
  }
  .secondary:hover {
    background: #d1d5db;
  }
  button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .msg {
    margin-top: 16px;
    padding: 12px;
    background: #f3f4f6;
    border-radius: 6px;
    white-space: pre-wrap;
    font-size: 0.85rem;
    color: #374151;
  }
</style>
