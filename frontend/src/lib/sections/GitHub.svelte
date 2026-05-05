<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open as openExternal } from '@tauri-apps/plugin-shell';

  let {
    username = $bindable(),
    showToast,
  }: {
    username: string | null;
    showToast: (kind: 'ok' | 'err' | 'info', text: string) => void;
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

  $effect(() => {
    if (username) {
      busy = false;
      userCode = '';
      verificationUri = '';
    }
  });

  // Belt-and-suspenders polling: while in device-flow state, also ask the
  // backend directly for the resolved username every few seconds. If the
  // background `github-login-complete` event somehow doesn't reach us
  // (race conditions in Tauri's async runtime, dropped task, etc.), this
  // fallback still picks up a successful auth.
  let pollHandle: ReturnType<typeof setInterval> | null = null;
  $effect(() => {
    if (userCode && !pollHandle) {
      pollHandle = setInterval(async () => {
        try {
          const u = await invoke<string | null>('get_github_username');
          if (u) {
            username = u;
            showToast('ok', `GitHub connected as ${u}`);
            if (pollHandle) {
              clearInterval(pollHandle);
              pollHandle = null;
            }
          }
        } catch {}
      }, 3000);
    } else if (!userCode && pollHandle) {
      clearInterval(pollHandle);
      pollHandle = null;
    }

    return () => {
      if (pollHandle) {
        clearInterval(pollHandle);
        pollHandle = null;
      }
    };
  });
</script>

{#if username}
  <div class="connected">
    <span class="check">✓</span>
    Connected as <strong>{username}</strong>
    <button class="link" onclick={disconnect}>Disconnect</button>
  </div>
{:else if userCode}
  <div class="device-flow">
    <p>
      Open
      <a href="#" onclick={(e) => { e.preventDefault(); openExternal(verificationUri); }}>
        {verificationUri}
      </a>
      and enter:
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
    background: var(--accent);
    color: #fff;
    border: 1px solid var(--accent);
    padding: 8px 18px;
    border-radius: 6px;
    font-size: 0.88rem;
    font-weight: 600;
    cursor: pointer;
  }
  .connect:hover { background: var(--accent-hover); border-color: var(--accent-hover); }
  .connect:disabled { opacity: 0.5; cursor: not-allowed; }

  .connected {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text);
    font-size: 0.9rem;
    padding: 8px 0;
  }
  .check {
    color: var(--accent);
    font-weight: 700;
  }
  .link {
    background: none;
    border: none;
    color: var(--text-muted);
    text-decoration: underline;
    cursor: pointer;
    font-size: 0.82rem;
    margin-left: auto;
  }
  .link:hover { color: var(--text); }

  .device-flow p {
    margin: 0 0 8px;
    color: var(--text-muted);
    font-size: 0.88rem;
  }
  .device-flow a {
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
    font-family: 'SF Mono', monospace;
  }
  .code {
    display: inline-block;
    padding: 10px 18px;
    background: #0f172a;
    color: #fff;
    font-size: 1.3rem;
    font-family: 'SF Mono', monospace;
    letter-spacing: 0.12em;
    border-radius: 6px;
    margin: 4px 0 8px;
  }
  .waiting {
    color: var(--text-faint) !important;
    font-style: italic;
  }
</style>
