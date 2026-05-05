<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open as openExternal } from '@tauri-apps/plugin-shell';
  import { onMount } from 'svelte';

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
  let repo = $state('');
  let repoSaving = $state(false);

  onMount(async () => {
    try {
      repo = await invoke<string>('get_github_repo');
    } catch {}
  });

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

  async function saveRepo() {
    repoSaving = true;
    try {
      await invoke('set_github_repo', { repo });
      showToast('ok', 'Repository name saved.');
    } catch (e) {
      showToast('err', `${e}`);
    } finally {
      repoSaving = false;
    }
  }

  $effect(() => {
    if (username) {
      busy = false;
      userCode = '';
      verificationUri = '';
    }
  });

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

  <div class="repo-block">
    <p class="repo-hint">
      Pipelines you upload will go to here. Change this if you want a
      different repository name.
    </p>
    <div class="repo-row">
      <span class="repo-prefix">github.com/{username}/</span>
      <input
        class="repo-input"
        bind:value={repo}
        placeholder="autopipe-hub"
      />
      <button class="ghost" disabled={repoSaving} onclick={saveRepo}>
        {repoSaving ? 'Saving…' : 'Save'}
      </button>
    </div>
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
  .check { color: var(--accent); font-weight: 700; }
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

  /* ── Repo input row ────────────────────────────── */
  .repo-block {
    margin-top: 14px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }
  .repo-label {
    display: block;
    font-size: 0.82rem;
    color: var(--text-muted);
    font-weight: 500;
    margin-bottom: 6px;
  }
  .repo-row {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    font-family: 'SF Mono', Menlo, monospace;
    font-size: 0.85rem;
  }
  .repo-prefix, .repo-suffix {
    color: var(--text-muted);
  }
  .repo-input {
    padding: 6px 10px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    font-size: 0.85rem;
    font-family: inherit;
    color: var(--text);
    background: var(--bg-card);
    min-width: 160px;
  }
  .repo-input:focus {
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
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
  }
  .ghost:hover { background: var(--accent-light); }
  .ghost:disabled { opacity: 0.6; cursor: not-allowed; }
  .repo-hint {
    margin: 0 0 8px;
    font-size: 0.8rem;
    color: var(--text-muted);
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
  }

  /* ── Device flow ──────────────────────────────── */
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
