<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { open as openExternal } from '@tauri-apps/plugin-shell';
  import { onMount } from 'svelte';
  import SshSection from '$lib/sections/Ssh.svelte';
  import GitHubSection from '$lib/sections/GitHub.svelte';
  import AdvancedPanel from '$lib/sections/AdvancedPanel.svelte';
  import PluginsPanel from '$lib/sections/PluginsPanel.svelte';

  type SshConfig = {
    host: string;
    port: number;
    user: string;
    auth_method: string; // 'password' | 'key'
    password: string;
    key_path: string;
    repo_path: string;
    connection_type: string; // 'ssh' | 'cloud'
    cloud_provider: string; // 'aws' | 'gcp' | 'azure'
  };

  let sshConfig = $state<SshConfig>({
    host: '',
    port: 22,
    user: '',
    auth_method: 'password',
    password: '',
    key_path: '',
    repo_path: '',
    connection_type: 'ssh',
    cloud_provider: 'aws',
  });

  // Switching to Cloud defaults to key auth (cloud VMs are key-based) and
  // ensures a provider is selected; switching back to SSH is left as-is.
  function setConnectionType(kind: 'ssh' | 'cloud') {
    sshConfig.connection_type = kind;
    if (kind === 'cloud') {
      if (!sshConfig.cloud_provider) sshConfig.cloud_provider = 'aws';
      if (sshConfig.auth_method !== 'key') sshConfig.auth_method = 'key';
    }
  }

  // ── AWS account connection (Phase 1: verify credentials + list buckets) ──
  const AWS_REGIONS: { group: string; items: [string, string][] }[] = [
    { group: '미국 US', items: [['us-east-1', '버지니아 북부'], ['us-east-2', '오하이오'], ['us-west-1', '캘리포니아 북부'], ['us-west-2', '오레곤']] },
    { group: '아시아 태평양 Asia Pacific', items: [['ap-south-1', '뭄바이'], ['ap-northeast-3', '오사카'], ['ap-northeast-2', '서울'], ['ap-southeast-1', '싱가포르'], ['ap-southeast-2', '시드니'], ['ap-northeast-1', '도쿄']] },
    { group: '캐나다 Canada', items: [['ca-central-1', '중부']] },
    { group: '유럽 Europe', items: [['eu-central-1', '프랑크푸르트'], ['eu-west-1', '아일랜드'], ['eu-west-2', '런던'], ['eu-west-3', '파리'], ['eu-north-1', '스톡홀름']] },
    { group: '남아메리카 South America', items: [['sa-east-1', '상파울루']] },
  ];

  let awsAccessKey = $state('');
  let awsSecretKey = $state('');
  let awsRegion = $state('us-east-1');
  let awsBucket = $state('');
  let awsAccount = $state<string | null>(null);
  let awsBuckets = $state<string[]>([]);
  let awsBusy = $state(false);
  let awsHasCreds = $state(false);

  async function awsConnect() {
    if (!awsAccessKey.trim() || !awsSecretKey.trim()) {
      showToast('err', 'Enter your AWS access key and secret.');
      return;
    }
    awsBusy = true;
    try {
      const account = await invoke<string>('aws_connect', {
        accessKey: awsAccessKey.trim(),
        secretKey: awsSecretKey.trim(),
        region: awsRegion.trim() || 'us-east-1',
      });
      awsAccount = account;
      awsHasCreds = true;
      showToast('ok', `AWS connected (account ${account}).`);
      await awsLoadBuckets();
    } catch (e) {
      awsAccount = null;
      showToast('err', `AWS connect failed: ${e}`);
    } finally {
      awsBusy = false;
    }
  }

  async function awsLoadBuckets() {
    awsBusy = true;
    try {
      awsBuckets = await invoke<string[]>('aws_list_buckets');
    } catch (e) {
      showToast('err', `List buckets failed: ${e}`);
    } finally {
      awsBusy = false;
    }
  }

  async function awsSelectBucket(b: string) {
    awsBucket = b;
    try {
      await invoke('aws_set_bucket', { bucket: b });
    } catch (e) {
      showToast('err', `${e}`);
    }
  }

  let githubUsername = $state<string | null>(null);
  let busy = $state(false);
  let toast = $state<{ kind: 'ok' | 'err' | 'info'; text: string } | null>(null);
  let showAdvanced = $state(false);
  let showPlugins = $state(false);
  let showVerify = $state(false);

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
    try {
      const a = await invoke<{ region: string; bucket: string; has_credentials: boolean }>('aws_get_config');
      if (a.region) awsRegion = a.region;
      awsBucket = a.bucket ?? '';
      awsHasCreds = a.has_credentials;
    } catch {}
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
      // The port field is a text input, so its bound value is a string.
      // Normalise here rather than relying on the backend to coerce it.
      const port = Number(String(sshConfig.port).trim());
      if (!Number.isInteger(port) || port < 1 || port > 65535) {
        showToast('err', 'SSH port must be a number between 1 and 65535.');
        busy = false;
        return;
      }
      if (sshConfig.auth_method === 'key' && !sshConfig.key_path.trim()) {
        showToast('err', 'Key file path is required when using SSH key auth.');
        busy = false;
        return;
      }
      await invoke('save_ssh_config', { config: { ...sshConfig, port } });
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
      <img src="/logo.png" alt="" class="brand-logo" />
      <div class="brand-text">
        <div class="logo">AutoPipe</div>
        <div class="tagline">
          Generate, execute, visualize, and share reproducible containerized pipelines with AI.
        </div>
      </div>
    </div>
    <div class="topbar-actions">
      <button class="stack-btn" title="Advanced settings" onclick={() => (showAdvanced = true)}>
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
        <span class="stack-label">Advanced</span>
      </button>
      <button class="stack-btn" title="Manage viewer plugins" onclick={() => (showPlugins = true)}>
        <!-- plug icon (plugins) -->
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M9 2v6" />
          <path d="M15 2v6" />
          <path d="M12 17v5" />
          <path d="M5 8h14" />
          <path d="M6 11V8h12v3a6 6 0 0 1-12 0Z" />
        </svg>
        <span class="stack-label">Plugins</span>
      </button>
    </div>
  </header>

  <main class="content">
    <section class="intro">
      <h1>Setup</h1>
      <p class="lead">
        Configure AutoPipe with the short steps below.
      </p>
      <p class="lead">
        If you're not sure how to configure things, see the detailed
        guide at
        <a href="#" onclick={(e) => { e.preventDefault(); openDocs(); }}>
          autopipe.org/getting-started</a>.
      </p>
    </section>

    <section class="step">
      <header class="step-head">
        <span class="num">1</span>
        <h2>Analysis machine</h2>
        <span class="badge required">Required</span>
      </header>
      <p class="step-desc">
        The machine where AutoPipe runs your analyses. Connect a self-managed
        Linux server over SSH, or a VM in a cloud provider.
      </p>

      <div class="conn-toggle" role="tablist" aria-label="Connection type">
        <button
          role="tab"
          class="conn-tab"
          class:active={sshConfig.connection_type !== 'cloud'}
          aria-selected={sshConfig.connection_type !== 'cloud'}
          onclick={() => setConnectionType('ssh')}
        >SSH server</button>
        <button
          role="tab"
          class="conn-tab"
          class:active={sshConfig.connection_type === 'cloud'}
          aria-selected={sshConfig.connection_type === 'cloud'}
          onclick={() => setConnectionType('cloud')}
        >Cloud VM</button>
      </div>

      {#if sshConfig.connection_type === 'cloud'}
        <div class="provider-row">
          <span class="provider-label">Provider</span>
          <select bind:value={sshConfig.cloud_provider}>
            <option value="aws">Amazon Web Services (EC2)</option>
            <option value="gcp">Google Cloud (Compute Engine)</option>
            <option value="azure">Microsoft Azure (VM)</option>
          </select>
        </div>
        {#if sshConfig.cloud_provider === 'aws'}
          <div class="aws-card">
            <div class="aws-head">
              <span class="aws-title">AWS account</span>
              {#if awsAccount}
                <span class="aws-badge ok">Connected · {awsAccount}</span>
              {:else if awsHasCreds}
                <span class="aws-badge">Saved — click Connect to verify</span>
              {/if}
            </div>
            <div class="aws-form">
              <label><span>Access Key ID</span><input bind:value={awsAccessKey} placeholder="AKIA…" /></label>
              <label>
                <span>Secret Access Key</span>
                <input type="password" bind:value={awsSecretKey} placeholder={awsHasCreds ? '•••••• (saved)' : ''} />
              </label>
              <label>
                <span>Region</span>
                <select class="aws-select" bind:value={awsRegion}>
                  {#each AWS_REGIONS as g}
                    <optgroup label={g.group}>
                      {#each g.items as [code, name]}
                        <option value={code}>{name} ({code})</option>
                      {/each}
                    </optgroup>
                  {/each}
                </select>
              </label>
            </div>
            <div class="aws-row">
              <button class="btn-outline small" disabled={awsBusy} onclick={awsConnect}>
                {awsBusy ? 'Connecting…' : 'Connect'}
              </button>
              <span class="aws-bucket-label">S3 bucket</span>
              <select
                class="aws-select"
                bind:value={awsBucket}
                onchange={(e) => awsSelectBucket((e.currentTarget as HTMLSelectElement).value)}
                disabled={awsBuckets.length === 0}
              >
                {#if awsBuckets.length === 0}
                  <option value={awsBucket}>{awsBucket || (awsAccount ? '(no buckets — create one in S3, then Refresh)' : '(connect to list buckets)')}</option>
                {:else}
                  <option value="">— select —</option>
                  {#each awsBuckets as b}
                    <option value={b}>{b}</option>
                  {/each}
                {/if}
              </select>
              <button class="btn-outline small" disabled={awsBusy} onclick={awsLoadBuckets}>Refresh</button>
            </div>
            <p class="aws-note">
              Phase 1: verifies your AWS credentials and lists your S3 buckets.
              Automatic VM provisioning &amp; S3-mounted execution come in the next update.
            </p>
          </div>
        {:else}
          <p class="conn-hint">
            Create a VM in your cloud, then paste its public IP, login user, and
            key file (.pem) below. Cloud VMs use key authentication.
          </p>
        {/if}
      {/if}

      <button
        class="verify-toggle"
        onclick={() => (showVerify = !showVerify)}
        aria-expanded={showVerify}
      >
        <span class="chevron" class:open={showVerify}>▸</span>
        To install everything AutoPipe needs on that machine, or if you're
        not sure what values to enter below, click here.
      </button>
      {#if showVerify}
        <div class="cmd-box">
          <p>
            Run this on the analysis machine (per-OS instructions:
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
      {/if}

      <SshSection bind:config={sshConfig} />
    </section>

    <section class="step">
      <header class="step-head">
        <span class="num">2</span>
        <h2>GitHub connection</h2>
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

    <section class="step">
      <header class="step-head">
        <span class="num">3</span>
        <h2>Save and Register</h2>
      </header>
      <p class="step-desc">
        When you're done with the steps above, click
        <strong>Save and Register</strong> at the bottom to save your
        settings and register AutoPipe with your AI app.
      </p>
      <div class="callout step-callout">
        Keep AutoPipe running in the tray while you use it.
        Use <strong>Move to tray</strong> instead of closing the window.
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
  {#if showPlugins}
    <PluginsPanel onclose={() => (showPlugins = false)} {showToast} />
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
  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .brand-logo {
    width: 40px;
    height: 40px;
    object-fit: contain;
    background: #ffffff;
    border-radius: 8px;
    padding: 4px;
    flex-shrink: 0;
  }
  .brand-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
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
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* Vertical stack of two icon-with-label buttons in the top-right. */
  .topbar-actions {
    display: flex;
    gap: 12px;
  }
  .stack-btn {
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 4px 8px 2px;
    background: transparent;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: 8px;
    cursor: pointer;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
    font-family: inherit;
  }
  .stack-btn:hover {
    background: var(--bg-soft);
    color: var(--accent);
    border-color: var(--accent);
  }
  .stack-label {
    font-size: 0.7rem;
    font-weight: 500;
    line-height: 1;
    letter-spacing: 0.01em;
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

  /* ── Connection type toggle (SSH vs Cloud) ─────────── */
  .conn-toggle {
    display: inline-flex;
    gap: 4px;
    margin: 0 0 14px 34px;
    padding: 3px;
    background: var(--bg-soft);
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .conn-tab {
    padding: 6px 16px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 0.85rem;
    font-weight: 500;
    border-radius: 6px;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.15s, color 0.15s;
  }
  .conn-tab:hover { color: var(--accent); }
  .conn-tab.active {
    background: var(--bg-card);
    color: var(--accent);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
  }
  .provider-row {
    display: flex;
    align-items: center;
    gap: 14px;
    margin: 0 0 10px 34px;
  }
  .provider-label {
    width: 100px;
    font-size: 0.85rem;
    color: var(--text-muted);
    font-weight: 500;
  }
  .provider-row select {
    flex: 1;
    padding: 7px 10px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    font-size: 0.9rem;
    background: var(--bg-card);
    color: var(--text);
    font-family: inherit;
  }
  .provider-row select:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-light);
  }
  .conn-hint {
    margin: 0 0 12px 34px;
    font-size: 0.82rem;
    color: var(--text-muted);
    line-height: 1.5;
  }

  /* ── AWS account card ──────────────────────────────── */
  .aws-card {
    margin: 0 0 14px 34px;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-soft);
  }
  .aws-head {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 10px;
  }
  .aws-title {
    font-size: 0.9rem;
    font-weight: 600;
    color: var(--text);
  }
  .aws-badge {
    font-size: 0.72rem;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 10px;
    border: 1px solid var(--border-strong);
    color: var(--text-faint);
  }
  .aws-badge.ok {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--accent-light);
  }
  .aws-form {
    display: grid;
    grid-template-columns: 130px 1fr;
    gap: 8px 12px;
    align-items: center;
  }
  .aws-form label { display: contents; }
  .aws-form label > span {
    font-size: 0.83rem;
    color: var(--text-muted);
    font-weight: 500;
  }
  .aws-form input,
  .aws-select {
    padding: 7px 10px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    font-size: 0.9rem;
    background: var(--bg-card);
    color: var(--text);
    font-family: inherit;
  }
  .aws-form input:focus,
  .aws-select:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-light);
  }
  .aws-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 12px;
    flex-wrap: wrap;
  }
  .aws-bucket-label {
    font-size: 0.83rem;
    color: var(--text-muted);
    font-weight: 500;
    margin-left: 6px;
  }
  .aws-select { flex: 1; min-width: 160px; }
  .btn-outline.small {
    padding: 6px 14px;
    font-size: 0.82rem;
  }
  .aws-note {
    margin: 12px 0 0;
    font-size: 0.78rem;
    color: var(--text-faint);
    line-height: 1.5;
  }

  /* ── Verify toggle + command callout (inside SSH step) ─ */
  .verify-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 12px 34px;
    padding: 6px 10px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-muted);
    font-size: 0.83rem;
    cursor: pointer;
    text-align: left;
    width: calc(100% - 34px);
    font-family: inherit;
  }
  .verify-toggle:hover {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--accent-light);
  }
  .chevron {
    display: inline-block;
    transition: transform 0.15s;
    color: var(--text-faint);
    font-size: 0.7rem;
  }
  .chevron.open { transform: rotate(90deg); color: var(--accent); }

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

  /* The callout inside the Step 3 card needs left indent matching others */
  .step-callout {
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
  /* Toast in the sticky bar — visually identical to .actionbar-hint
     (same size, same muted color, no border) so it doesn't fight for
     attention. The leading icon hints at the kind without color noise. */
  .toast {
    flex: 1;
    display: flex;
    gap: 6px;
    align-items: flex-start;
    margin: 0;
    padding: 0;
    border: none;
    background: transparent;
    font-size: 0.83rem;
    line-height: 1.45;
    color: var(--text-muted);
  }
  .toast.err { color: var(--danger); }
  .toast > span { flex: 1; }

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
