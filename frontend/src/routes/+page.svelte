<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { open as openExternal } from '@tauri-apps/plugin-shell';
  import { onMount } from 'svelte';
  import SshSection from '$lib/sections/Ssh.svelte';
  import GitHubSection from '$lib/sections/GitHub.svelte';
  import AdvancedPanel from '$lib/sections/AdvancedPanel.svelte';

  type SshConfig = {
    host: string;
    port: number;
    user: string;
    password: string;
    repo_path: string;
  };

  let sshConfig = $state<SshConfig>({
    host: '',
    port: 22,
    user: '',
    password: '',
    repo_path: '',
  });

  let githubUsername = $state<string | null>(null);
  let busy = $state(false);
  let toast = $state<{ kind: 'ok' | 'err' | 'info'; text: string } | null>(null);
  let showAdvanced = $state(false);

  function showToast(kind: 'ok' | 'err' | 'info', text: string) {
    toast = { kind, text };
    setTimeout(() => {
      if (toast?.text === text) toast = null;
    }, 6000);
  }

  function openDocs() {
    openExternal('https://autopipe.org/getting-started').catch(() => {});
  }

  onMount(async () => {
    try { sshConfig = await invoke<SshConfig>('get_ssh_config'); } catch {}
    try { githubUsername = await invoke<string | null>('get_github_username'); } catch {}
    await listen<string | null>('github-login-complete', (event) => {
      githubUsername = event.payload;
      showToast('ok', `GitHub connected as ${event.payload}`);
    });
    await listen<string>('github-login-error', (event) => {
      showToast('err', `GitHub login failed: ${event.payload}`);
    });
  });

  async function saveAndRegister() {
    busy = true;
    try {
      if (!sshConfig.host.trim() || !sshConfig.user.trim()) {
        showToast('err', 'SSH host and user are required.');
        busy = false;
        return;
      }
      await invoke('save_ssh_config', { config: sshConfig });
      await invoke<string[]>('register_mcp');
      const ghMsg = githubUsername
        ? ''
        : ' GitHub is not connected — you can run public pipelines but cannot upload your own.';
      showToast(
        'ok',
        `Saved.${ghMsg} Restart your AI app, then click Move to tray to keep AutoPipe running.`
      );
    } catch (e) {
      showToast('err', `Failed: ${e}`);
    } finally {
      busy = false;
    }
  }

  async function moveToTray() {
    try {
      await invoke('move_to_tray');
    } catch (e) {
      showToast('err', `Failed: ${e}`);
    }
  }
</script>

<div class="page">
  <header class="topbar">
    <div class="brand">
      <div class="logo">AutoPipe</div>
      <div class="tagline">Bioinformatics pipelines, in your AI chat.</div>
    </div>
    <button class="icon-btn" title="Advanced" onclick={() => (showAdvanced = true)}>
      <!-- sliders icon (advanced settings) -->
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <line x1="4" y1="21" x2="4" y2="14" />
        <line x1="4" y1="10" x2="4" y2="3" />
        <line x1="12" y1="21" x2="12" y2="12" />
        <line x1="12" y1="8" x2="12" y2="3" />
        <line x1="20" y1="21" x2="20" y2="16" />
        <line x1="20" y1="12" x2="20" y2="3" />
        <line x1="1" y1="14" x2="7" y2="14" />
        <line x1="9" y1="8" x2="15" y2="8" />
        <line x1="17" y1="16" x2="23" y2="16" />
      </svg>
    </button>
  </header>

  <main class="content">
    <section class="intro">
      <h1>Setup</h1>
      <p class="lead">
        Configure AutoPipe with the short steps below, then click
        <strong>Save and Register</strong> at the bottom.
      </p>
      <p class="lead">
        Only the SSH step is required — you can skip GitHub and still find
        and run public pipelines. If you're not sure how to configure
        things, see the detailed guide at
        <a href="#" onclick={(e) => { e.preventDefault(); openDocs(); }}>
          autopipe.org/getting-started</a>.
      </p>
      <div class="callout">
        Keep AutoPipe in the tray while using it from your AI app —
        AutoPipe needs to be running for your AI app to talk to it.
        Use <strong>Move to tray</strong> below instead of quitting.
      </div>
    </section>

    <section class="step">
      <header class="step-head">
        <span class="num">1</span>
        <h2>SSH server</h2>
        <span class="badge required">Required</span>
      </header>
      <p class="step-desc">The machine that will run your analyses.</p>

      <div class="cmd-box">
        <p>
          To verify your machine is ready and get the values to enter below,
          run this command on that machine
          (per-OS instructions:
          <a href="#" onclick={(e) => { e.preventDefault(); openDocs(); }}>
            autopipe.org/getting-started</a>):
        </p>
        <div class="cmd-row">
          <code>curl -fsSL https://download.autopipe.org/setup.sh | bash</code>
          <button
            class="ghost small"
            onclick={() => {
              navigator.clipboard.writeText(
                'curl -fsSL https://download.autopipe.org/setup.sh | bash'
              );
              showToast('ok', 'Command copied.');
            }}
          >Copy</button>
        </div>
      </div>

      <SshSection bind:config={sshConfig} />
    </section>

    <section class="step">
      <header class="step-head">
        <span class="num">2</span>
        <h2>GitHub</h2>
        <span class="badge optional">Optional</span>
      </header>
      <p class="step-desc">
        Only needed to upload or publish your own pipelines. Skip if you
        only want to run pipelines from AutoPipeHub.
      </p>
      <div class="github-row">
        <GitHubSection bind:username={githubUsername} {showToast} />
      </div>
    </section>
  </main>

  <footer class="actionbar">
    <div class="actionbar-inner">
      {#if toast}
        <div class="toast" class:ok={toast.kind === 'ok'} class:err={toast.kind === 'err'} class:info={toast.kind === 'info'}>
          {#if toast.kind === 'ok'}✓{:else if toast.kind === 'err'}✗{:else}ⓘ{/if}
          <span>{toast.text}</span>
        </div>
      {:else}
        <p class="actionbar-hint">
          Keep AutoPipe in the tray to let your AI app use it.
        </p>
      {/if}
      <button class="btn-outline" onclick={moveToTray}>Move to tray</button>
      <button class="btn-primary" disabled={busy} onclick={saveAndRegister}>
        {busy ? 'Saving…' : 'Save and Register'}
      </button>
    </div>
  </footer>

  {#if showAdvanced}
    <AdvancedPanel onclose={() => (showAdvanced = false)} {showToast} />
  {/if}
</div>

<style>
  .page {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
  }

  /* ── Top bar ──────────────────────────────────────── */
  .topbar {
    background: var(--bg-card);
    border-bottom: 1px solid var(--border);
    padding: 16px 32px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    max-width: 100%;
  }
  .brand .logo {
    font-size: 1.3rem;
    font-weight: 700;
    color: var(--accent);
    line-height: 1.2;
    letter-spacing: -0.01em;
  }
  .brand .tagline {
    font-size: 0.82rem;
    color: var(--text-muted);
    line-height: 1.3;
    margin-top: 2px;
  }
  .icon-btn {
    width: 36px;
    height: 36px;
    border-radius: 8px;
    background: transparent;
    color: var(--text-muted);
    border: 1px solid var(--border);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    transition: background 0.15s, color 0.15s;
  }
  .icon-btn:hover {
    background: var(--bg-soft);
    color: var(--accent);
    border-color: var(--accent);
  }

  /* ── Content ──────────────────────────────────────── */
  .content {
    flex: 1;
    width: 100%;
    max-width: 800px;
    margin: 0 auto;
    padding: 28px 32px 130px;
  }
  .intro {
    margin-bottom: 22px;
  }
  .intro h1 {
    margin: 0 0 12px;
    font-size: 1.6rem;
    font-weight: 700;
    color: var(--text);
    letter-spacing: -0.02em;
  }
  .lead {
    margin: 0 0 10px;
    font-size: 0.95rem;
    color: var(--text);
    line-height: 1.55;
  }
  .lead a {
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
  }
  .callout {
    margin-top: 14px;
    padding: 12px 14px;
    border-left: 3px solid var(--accent);
    background: var(--accent-light);
    color: var(--text);
    font-size: 0.88rem;
    line-height: 1.5;
    border-radius: 4px;
  }

  /* ── Step cards ──────────────────────────────────── */
  .step {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 18px 22px 22px;
    margin-bottom: 16px;
  }
  .step-head {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 4px;
  }
  .num {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 1.5px solid var(--accent);
    color: var(--accent);
    font-size: 0.78rem;
    font-weight: 700;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .step-head h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
    color: var(--text);
    flex: 1;
  }
  .step-desc {
    margin: 0 0 14px 34px;
    font-size: 0.85rem;
    color: var(--text-muted);
    line-height: 1.5;
  }
  .badge {
    font-size: 0.68rem;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 10px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    background: transparent;
    border: 1px solid;
  }
  .badge.required {
    color: var(--accent);
    border-color: var(--accent);
  }
  .badge.optional {
    color: var(--text-faint);
    border-color: var(--border-strong);
  }

  /* ── Command callout (inside SSH step) ─────────────── */
  .cmd-box {
    margin: 0 0 16px 34px;
    padding: 12px 14px;
    border-left: 3px solid var(--accent);
    background: var(--bg-soft);
    border-radius: 4px;
  }
  .cmd-box p {
    margin: 0 0 8px;
    font-size: 0.83rem;
    color: var(--text-muted);
    line-height: 1.5;
  }
  .cmd-box a {
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
  }
  .cmd-row {
    display: flex;
    align-items: center;
    gap: 8px;
    background: #0f172a;
    color: #e2e8f0;
    padding: 8px 12px;
    border-radius: 4px;
    font-family: 'SF Mono', Menlo, monospace;
    font-size: 0.8rem;
  }
  .cmd-row code {
    flex: 1;
    overflow-x: auto;
    white-space: nowrap;
  }

  /* GitHub button row — slight indent under "2 GitHub" title */
  .github-row {
    margin-left: 34px;
  }

  /* ── Sticky action bar ───────────────────────────── */
  .actionbar {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    background: var(--bg-card);
    border-top: 1px solid var(--border);
    padding: 14px 32px;
    z-index: 20;
  }
  .actionbar-inner {
    max-width: 800px;
    margin: 0 auto;
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .actionbar-hint {
    flex: 1;
    margin: 0;
    font-size: 0.83rem;
    color: var(--text-muted);
  }
  .toast {
    flex: 1;
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 8px 12px;
    border: 1px solid var(--border-strong);
    background: var(--bg-card);
    border-radius: 6px;
    font-size: 0.85rem;
    line-height: 1.45;
    color: var(--text);
  }
  .toast.ok { border-color: var(--accent); color: var(--accent); }
  .toast.err { border-color: var(--danger); color: var(--danger); }
  .toast.info { border-color: var(--border-strong); color: var(--text-muted); }
  .toast > span { flex: 1; color: var(--text); }

  /* ── Buttons ─────────────────────────────────────── */
  .btn-primary {
    background: var(--accent);
    color: #fff;
    border: 1px solid var(--accent);
    border-radius: 6px;
    padding: 9px 22px;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-primary:hover { background: var(--accent-hover); border-color: var(--accent-hover); }
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }

  .btn-outline {
    background: transparent;
    color: var(--accent);
    border: 1px solid var(--accent);
    border-radius: 6px;
    padding: 9px 18px;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-outline:hover { background: var(--accent-light); }

  .ghost.small {
    background: transparent;
    color: #cbd5e1;
    border: 1px solid #475569;
    border-radius: 4px;
    padding: 4px 10px;
    font-size: 0.75rem;
    cursor: pointer;
  }
  .ghost.small:hover { background: #1e293b; color: #fff; }
</style>
