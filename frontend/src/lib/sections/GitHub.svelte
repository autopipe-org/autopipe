<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open as openExternal } from '@tauri-apps/plugin-shell';

  let {
    username = $bindable(),
    showToast,
  }: {
    username: string | null;
    showToast: (kind: 'ok' | 'err', text: string) => void;
  } = $props();

  let busy = $state(false);
  let userCode = $state('');
  let verificationUri = $state('');

  async function startLogin() {
    busy = true;
    try {
      const flow = await invoke<{
        user_code: string;
        verification_uri: string;
      }>('start_github_login');
      userCode = flow.user_code;
      verificationUri = flow.verification_uri;
      // Try to open the verification URL in the browser
      try {
        await openExternal(flow.verification_uri);
      } catch {}
    } catch (e) {
      showToast('err', `GitHub login failed: ${e}`);
      busy = false;
    }
  }

  async function disconnect() {
    try {
      await invoke('clear_github_token');
      username = null;
      userCode = '';
      verificationUri = '';
      showToast('ok', 'Disconnected from GitHub.');
    } catch (e) {
      showToast('err', `Failed: ${e}`);
    }
  }

  // When parent receives github-login-complete, username updates and we
  // can clear the in-progress state.
  $effect(() => {
    if (username) {
      busy = false;
      userCode = '';
      verificationUri = '';
    }
  });
</script>

{#if username}
  <div class="status connected">
    Connected as <strong>{username}</strong>
    <button class="link" onclick={disconnect}>Disconnect</button>
  </div>
{:else if userCode}
  <div class="device-flow">
    <p>
      Open this URL and enter the code:
    </p>
    <p class="url">
      <a href="#" onclick={(e) => { e.preventDefault(); openExternal(verificationUri); }}>
        {verificationUri}
      </a>
    </p>
    <pre class="code">{userCode}</pre>
    <p class="waiting">Waiting for you to authorize…</p>
  </div>
{:else}
  <button class="connect" disabled={busy} onclick={startLogin}>
    {busy ? 'Starting…' : 'Connect GitHub'}
  </button>
{/if}

<style>
  .connect {
    background: #24292e;
    color: #fff;
    border: none;
    padding: 10px 18px;
    border-radius: 6px;
    font-size: 0.9rem;
    font-weight: 500;
    cursor: pointer;
  }
  .connect:hover {
    background: #1a1e22;
  }
  .connect:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-radius: 6px;
    background: #d1fae5;
    color: #065f46;
    font-size: 0.9rem;
  }
  .link {
    background: none;
    border: none;
    color: inherit;
    text-decoration: underline;
    cursor: pointer;
    font-size: 0.85rem;
    margin-left: auto;
  }
  .device-flow p {
    margin: 0 0 10px;
    color: #4b5563;
    font-size: 0.9rem;
  }
  .url a {
    color: #0f4c5c;
    text-decoration: underline;
    cursor: pointer;
    font-family: 'SF Mono', monospace;
  }
  .code {
    display: inline-block;
    padding: 12px 20px;
    background: #1a2332;
    color: #fff;
    font-size: 1.4rem;
    font-family: 'SF Mono', monospace;
    letter-spacing: 0.12em;
    border-radius: 8px;
    margin: 6px 0 10px;
  }
  .waiting {
    color: #9ca3af !important;
    font-style: italic;
  }
</style>
