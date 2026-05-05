<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { open as openExternal } from '@tauri-apps/plugin-shell';
  import { onMount } from 'svelte';
  import SshSection from '$lib/sections/Ssh.svelte';
  import GitHubSection from '$lib/sections/GitHub.svelte';
  import StatusSection from '$lib/sections/Status.svelte';

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
  let mcpExpanded = $state(false);

  function showToast(kind: 'ok' | 'err' | 'info', text: string) {
    toast = { kind, text };
    setTimeout(() => {
      if (toast?.text === text) toast = null;
    }, 6000);
  }

  onMount(async () => {
    try { sshConfig = await invoke<SshConfig>('get_ssh_config'); } catch {}
    try { githubUsername = await invoke<string | null>('get_github_username'); } catch {}
    await listen<string | null>('github-login-complete', (event) => {
      githubUsername = event.payload;
      showToast('ok', `GitHub connected as ${event.payload}`);
    });
  });

  async function saveAndRegister() {
    busy = true;
    try {
      // Validate SSH (only required field)
      if (!sshConfig.host.trim() || !sshConfig.user.trim()) {
        showToast('err', 'SSH host and user are required.');
        busy = false;
        return;
      }
      await invoke('save_ssh_config', { config: sshConfig });
      const clients = await invoke<string[]>('register_mcp');
      const ghMsg = githubUsername
        ? ''
        : ' GitHub not connected — you can run public pipelines but cannot upload your own.';
      if (clients.length > 0) {
        showToast('ok', `Saved. Registered in ${clients.join(', ')}.${ghMsg} Restart your AI app.`);
      } else {
        showToast('info', `Saved.${ghMsg} Install Claude Desktop or Codex CLI to use AutoPipe.`);
      }
    } catch (e) {
      showToast('err', `Failed: ${e}`);
    } finally {
      busy = false;
    }
  }
</script>

<div class="page">
  <header class="topbar">
    <div class="brand">
      <img src="/logo.svg" alt="" class="logo-img" onerror={(e) => (e.currentTarget.style.display = 'none')} />
      <div>
        <div class="logo">AutoPipe</div>
        <div class="tagline">Bioinformatics pipelines, in your AI chat.</div>
      </div>
    </div>
  </header>

  <main class="content">
    <section class="intro">
      <h1>Setup</h1>
      <p class="lead">
        Configure AutoPipe in three short steps below, then click
        <strong>Save and Register</strong>.
      </p>
      <p class="note">
        Only the <strong>SSH</strong> step is required. You can skip
        <strong>GitHub</strong> and still find and run public pipelines —
        connect later if you want to publish your own.
        Full guide:
        <a href="#" onclick={(e) => { e.preventDefault(); openExternal('https://autopipe.org/getting-started'); }}>
          autopipe.org/getting-started
        </a>.
      </p>
    </section>

    <section class="step">
      <header class="step-header">
        <div class="step-title">
          <span class="step-num">1</span>
          <h2>SSH server</h2>
          <span class="badge required">Required</span>
        </div>
        <p class="step-desc">Where AutoPipe will run your analyses.</p>
      </header>
      <div class="step-body">
        <SshSection bind:config={sshConfig} />
      </div>
    </section>

    <section class="step">
      <header class="step-header">
        <div class="step-title">
          <span class="step-num">2</span>
          <h2>GitHub</h2>
          <span class="badge optional">Optional</span>
        </div>
        <p class="step-desc">
          Only needed to upload or publish your own pipelines.
        </p>
      </header>
      <div class="step-body">
        <GitHubSection bind:username={githubUsername} {showToast} />
      </div>
    </section>

    <section class="step">
      <button
        class="step-header collapsible"
        onclick={() => (mcpExpanded = !mcpExpanded)}
        aria-expanded={mcpExpanded}
      >
        <div class="step-title">
          <span class="step-num">3</span>
          <h2>MCP server details</h2>
          <span class="badge auto">Auto</span>
        </div>
        <p class="step-desc">
          URL and token for AI app connection. Most users don't need this.
          <span class="chevron" class:open={mcpExpanded}>▸</span>
        </p>
      </button>
      {#if mcpExpanded}
        <div class="step-body">
          <StatusSection {showToast} />
        </div>
      {/if}
    </section>
  </main>

  <footer class="actionbar">
    <div class="actionbar-inner">
      {#if toast}
        <div class="toast" class:ok={toast.kind === 'ok'} class:err={toast.kind === 'err'} class:info={toast.kind === 'info'}>
          {toast.text}
        </div>
      {/if}
      <button class="primary" disabled={busy} onclick={saveAndRegister}>
        {busy ? 'Saving…' : 'Save and Register'}
      </button>
    </div>
  </footer>
</div>

<style>
  .page {
    min-height: 100vh;
    background: linear-gradient(180deg, #f8fafc 0%, #f1f5f9 100%);
    display: flex;
    flex-direction: column;
  }

  /* Top bar */
  .topbar {
    background: linear-gradient(135deg, #0f4c5c 0%, #1a6373 100%);
    color: #fff;
    padding: 18px 32px;
    box-shadow: 0 2px 6px rgba(15, 76, 92, 0.2);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
    max-width: 820px;
    margin: 0 auto;
  }
  .logo-img {
    width: 40px;
    height: 40px;
    border-radius: 8px;
  }
  .logo {
    font-size: 1.35rem;
    font-weight: 700;
    letter-spacing: 0.01em;
    line-height: 1.2;
  }
  .tagline {
    font-size: 0.8rem;
    opacity: 0.78;
    line-height: 1.2;
    margin-top: 2px;
  }

  /* Main content */
  .content {
    flex: 1;
    width: 100%;
    max-width: 820px;
    margin: 0 auto;
    padding: 36px 32px 120px;
    box-sizing: border-box;
  }

  /* Intro */
  .intro {
    margin-bottom: 28px;
  }
  .intro h1 {
    margin: 0 0 12px;
    font-size: 1.7rem;
    font-weight: 700;
    color: #0f172a;
    letter-spacing: -0.02em;
  }
  .lead {
    margin: 0 0 12px;
    font-size: 1rem;
    color: #334155;
    line-height: 1.55;
  }
  .note {
    margin: 0;
    font-size: 0.88rem;
    color: #64748b;
    line-height: 1.6;
    padding: 12px 16px;
    background: #fff;
    border-left: 3px solid #0f4c5c;
    border-radius: 4px;
  }
  .note a {
    color: #0f4c5c;
    text-decoration: underline;
  }

  /* Step cards */
  .step {
    background: #fff;
    border-radius: 12px;
    margin-bottom: 16px;
    box-shadow: 0 1px 3px rgba(15, 23, 42, 0.06), 0 1px 2px rgba(15, 23, 42, 0.04);
    overflow: hidden;
    transition: box-shadow 0.2s;
  }
  .step:hover {
    box-shadow: 0 4px 12px rgba(15, 23, 42, 0.08), 0 1px 3px rgba(15, 23, 42, 0.05);
  }
  .step-header {
    padding: 18px 24px 14px;
    border-bottom: 1px solid #f1f5f9;
    width: 100%;
    text-align: left;
    background: none;
    border-left: none;
    border-right: none;
    border-top: none;
    cursor: default;
    box-sizing: border-box;
  }
  .step-header.collapsible {
    cursor: pointer;
    border-bottom: none;
  }
  .step-header.collapsible:hover {
    background: #f8fafc;
  }
  .step-title {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 4px;
  }
  .step-num {
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: #0f4c5c;
    color: #fff;
    font-size: 0.8rem;
    font-weight: 700;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .step-title h2 {
    margin: 0;
    font-size: 1.05rem;
    font-weight: 600;
    color: #0f172a;
    flex: 1;
  }
  .step-desc {
    margin: 0 0 0 36px;
    font-size: 0.85rem;
    color: #64748b;
    line-height: 1.5;
  }

  /* Badges */
  .badge {
    font-size: 0.7rem;
    font-weight: 600;
    padding: 3px 9px;
    border-radius: 11px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .badge.required {
    background: #fef3c7;
    color: #92400e;
  }
  .badge.optional {
    background: #e0e7ff;
    color: #3730a3;
  }
  .badge.auto {
    background: #dbeafe;
    color: #1e40af;
  }

  /* Chevron for collapsible */
  .chevron {
    display: inline-block;
    margin-left: 6px;
    transition: transform 0.2s;
    color: #94a3b8;
  }
  .chevron.open {
    transform: rotate(90deg);
  }

  .step-body {
    padding: 18px 24px 22px;
  }

  /* Sticky action bar */
  .actionbar {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    background: rgba(255, 255, 255, 0.95);
    backdrop-filter: blur(8px);
    border-top: 1px solid #e2e8f0;
    padding: 14px 32px;
    box-shadow: 0 -4px 12px rgba(15, 23, 42, 0.06);
    z-index: 20;
  }
  .actionbar-inner {
    max-width: 820px;
    margin: 0 auto;
    display: flex;
    align-items: center;
    gap: 14px;
    justify-content: flex-end;
  }
  .primary {
    background: linear-gradient(135deg, #0f4c5c 0%, #1a6373 100%);
    color: #fff;
    border: none;
    border-radius: 8px;
    padding: 11px 26px;
    font-size: 0.95rem;
    font-weight: 600;
    cursor: pointer;
    box-shadow: 0 2px 6px rgba(15, 76, 92, 0.25);
    transition: transform 0.05s, box-shadow 0.15s;
  }
  .primary:hover {
    box-shadow: 0 4px 12px rgba(15, 76, 92, 0.3);
  }
  .primary:active {
    transform: translateY(1px);
  }
  .primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
    box-shadow: none;
  }

  .toast {
    flex: 1;
    padding: 9px 14px;
    border-radius: 6px;
    font-size: 0.85rem;
    line-height: 1.4;
    animation: slideIn 0.2s ease-out;
  }
  .toast.ok {
    background: #d1fae5;
    color: #065f46;
    border: 1px solid #6ee7b7;
  }
  .toast.err {
    background: #fee2e2;
    color: #991b1b;
    border: 1px solid #fca5a5;
  }
  .toast.info {
    background: #dbeafe;
    color: #1e40af;
    border: 1px solid #93c5fd;
  }
  @keyframes slideIn {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
</style>
