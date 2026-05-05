<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  let username = $state<string | null>(null);
  let busy = $state(false);
  let userCode = $state('');
  let verificationUri = $state('');
  let message = $state('');

  onMount(async () => {
    try {
      username = await invoke<string | null>('get_github_username');
    } catch {
      username = null;
    }
  });

  async function startLogin() {
    busy = true;
    message = '';
    try {
      const flow = await invoke<{ user_code: string; verification_uri: string }>(
        'start_github_login'
      );
      userCode = flow.user_code;
      verificationUri = flow.verification_uri;
      // Backend will poll and emit `github-login-complete`
    } catch (e) {
      message = `Error: ${e}`;
      busy = false;
    }
  }

  async function logout() {
    await invoke('clear_github_token');
    username = null;
    message = 'Logged out.';
  }
</script>

<section>
  <h2>GitHub</h2>
  {#if username}
    <p>Connected as <strong>{username}</strong>.</p>
    <button class="secondary" onclick={logout}>Disconnect</button>
  {:else if userCode}
    <p>
      Open <a href={verificationUri} target="_blank" rel="noopener">
        {verificationUri}
      </a> in your browser and enter this code:
    </p>
    <pre class="code">{userCode}</pre>
    <p class="hint">Waiting for authorization...</p>
  {:else}
    <p>Connect your GitHub account to enable pipeline upload and publish.</p>
    <button class="primary" disabled={busy} onclick={startLogin}>
      Connect GitHub
    </button>
  {/if}

  {#if message}<p class="msg">{message}</p>{/if}
</section>

<style>
  h2 { margin: 0 0 12px; font-size: 1.25rem; }
  p { color: #4b5563; }
  .code {
    display: inline-block;
    padding: 12px 24px;
    background: #1a2332;
    color: #fff;
    font-size: 1.5rem;
    font-family: 'SF Mono', monospace;
    letter-spacing: 0.1em;
    border-radius: 8px;
  }
  .hint { color: #9ca3af; font-size: 0.85rem; }
  button {
    padding: 10px 18px;
    border-radius: 6px;
    font-size: 0.9rem;
    font-weight: 500;
    border: none;
    cursor: pointer;
  }
  .primary { background: #0f4c5c; color: #fff; }
  .secondary { background: #e5e7eb; color: #1f2937; }
  button:disabled { opacity: 0.6; }
  .msg { color: #047857; font-size: 0.9rem; }
  a { color: #0f4c5c; }
</style>
