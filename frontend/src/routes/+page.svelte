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
    // Manual SSH is password-only; the AWS VM uses its own key internally
    // (aws_vm_key_path), independent of this field.
    sshConfig.auth_method = 'password';
    // Only AWS is supported for auto-provisioning, so the cloud tab is always AWS.
    if (kind === 'cloud') sshConfig.cloud_provider = 'aws';
  }

  // ── AWS account connection (Phase 1: verify credentials + list buckets) ──
  const AWS_REGIONS: { group: string; items: [string, string][] }[] = [
    { group: 'US', items: [['us-east-1', 'N. Virginia'], ['us-east-2', 'Ohio'], ['us-west-1', 'N. California'], ['us-west-2', 'Oregon']] },
    { group: 'Asia Pacific', items: [['ap-south-1', 'Mumbai'], ['ap-northeast-3', 'Osaka'], ['ap-northeast-2', 'Seoul'], ['ap-southeast-1', 'Singapore'], ['ap-southeast-2', 'Sydney'], ['ap-northeast-1', 'Tokyo']] },
    { group: 'Canada', items: [['ca-central-1', 'Central']] },
    { group: 'Europe', items: [['eu-central-1', 'Frankfurt'], ['eu-west-1', 'Ireland'], ['eu-west-2', 'London'], ['eu-west-3', 'Paris'], ['eu-north-1', 'Stockholm']] },
    { group: 'South America', items: [['sa-east-1', 'São Paulo']] },
  ];

  let awsAccessKey = $state('');
  let awsSecretKey = $state('');
  let awsRegion = $state('us-east-1');
  let awsBucket = $state('');
  let awsAccount = $state<string | null>(null);
  let awsBuckets = $state<string[]>([]);
  let awsBusy = $state(false);
  let awsHasCreds = $state(false);
  let awsUserName = $state('');

  // In-app modal so the paste instructions are actually seen before CloudShell opens.
  // A native confirm()/alert() is unreliable in the Tauri webview (no dialog on Linux),
  // so we render our own overlay and only open the browser when the user clicks OK.
  let awsSetupModal = $state(false);
  let awsSetupCmd = $state('');
  let cmdCopied = $state(false);

  async function copySetupCmd() {
    try {
      await navigator.clipboard.writeText(awsSetupCmd);
      cmdCopied = true;
      setTimeout(() => (cmdCopied = false), 2000);
    } catch {}
  }

  // Copy the 1-line setup command to the clipboard, then show the instruction modal.
  // The command grants this IAM user the permissions AutoPipe needs (username
  // auto-filled from the connected identity). CloudShell opens on OK, not before.
  async function awsSetupPermissions() {
    if (!awsUserName) {
      showToast('err', 'Connect your AWS account first.');
      return;
    }
    awsSetupCmd = `curl -fsSL https://download.autopipe.org/autopipe-aws-setup.sh | bash -s -- ${awsUserName}`;
    try {
      await navigator.clipboard.writeText(awsSetupCmd);
    } catch {}
    awsSetupModal = true;
  }

  // OK on the modal: open AWS CloudShell so the user can paste + run the command.
  function awsSetupOpenCloudShell() {
    awsSetupModal = false;
    const region = awsRegion || 'us-east-1';
    const url = `https://console.aws.amazon.com/cloudshell/home?region=${region}`;
    openExternal(url).catch(() => {});
  }

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
      try {
        const c = await invoke<{ user_name: string }>('aws_get_config');
        awsUserName = c.user_name ?? '';
      } catch {}
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

  // ── AWS VM provisioning + lifecycle (Phase 2 + Phase 4) ──
  let awsVm = $state<{ provisioned: boolean; instance_id: string; host: string }>({
    provisioned: false,
    instance_id: '',
    host: '',
  });
  let awsVmBusy = $state(false);
  // Live AWS lifecycle state: 'none' | 'running' | 'stopped' | 'pending' | 'stopping' | 'unknown'.
  let awsVmState = $state('none');
  // Confirm modal for impactful VM actions (stop / terminate), so the user sees
  // the cost/data consequences before it happens.
  let vmConfirm = $state<null | {
    title: string;
    lines: string[];
    okLabel: string;
    danger: boolean;
    run: () => Promise<void>;
  }>(null);

  async function refreshVmState() {
    try {
      awsVmState = await invoke<string>('aws_vm_state');
    } catch {
      awsVmState = 'unknown';
    }
  }

  async function awsProvision() {
    awsVmBusy = true;
    try {
      const r = await invoke<{ instance_id: string; public_ip: string }>('aws_provision');
      awsVm = { provisioned: true, instance_id: r.instance_id, host: r.public_ip };
      awsVmState = 'running';
      showToast('ok', `VM ready: ${r.instance_id} (${r.public_ip}). Click Save and Register to use it.`);
    } catch (e) {
      const msg = String(e);
      if (msg.includes('UnauthorizedOperation') || msg.includes('not authorized') || msg.includes('AccessDenied') || msg.includes('InstanceProfile')) {
        showToast('info', 'Missing AWS permissions. Copied a setup command and opened CloudShell; paste it there, press Enter, then click Provision VM again.');
        awsSetupPermissions();
      } else {
        showToast('err', `Provision failed: ${e}`);
      }
    } finally {
      awsVmBusy = false;
    }
  }

  async function awsStart() {
    awsVmBusy = true;
    try {
      const ip = await invoke<string>('aws_start_vm');
      awsVm = { ...awsVm, host: ip };
      awsVmState = 'running';
      showToast('ok', `VM started (${ip}). Its IP changed and AutoPipe updated it automatically.`);
    } catch (e) {
      showToast('err', `Start failed: ${e}`);
    } finally {
      awsVmBusy = false;
    }
  }

  function confirmStop() {
    vmConfirm = {
      title: 'Stop the VM?',
      okLabel: 'Stop VM',
      danger: false,
      lines: [
        'Keeps your disk and installed tools, so you can resume later.',
        'Storage-only billing while stopped, no compute charge.',
        'IP changes on restart; AutoPipe updates it automatically.',
      ],
      run: async () => {
        awsVmBusy = true;
        try {
          await invoke('aws_stop_vm');
          awsVmState = 'stopped';
          showToast('ok', 'VM stopped. Compute billing paused; disk is kept. Start it anytime.');
        } catch (e) {
          showToast('err', `Stop failed: ${e}`);
        } finally {
          awsVmBusy = false;
        }
      },
    };
  }

  function confirmTerminate() {
    vmConfirm = {
      title: 'Terminate the VM?',
      okLabel: 'Terminate VM',
      danger: true,
      lines: [
        'Permanently deletes the VM, its disk, tools, and data.',
        'Billing stops fully.',
        'To keep your setup and just pause billing, use Stop instead.',
      ],
      run: async () => {
        awsVmBusy = true;
        try {
          await invoke('aws_teardown');
          awsVm = { provisioned: false, instance_id: '', host: '' };
          awsVmState = 'none';
          showToast('ok', 'VM terminated and cleaned up.');
        } catch (e) {
          showToast('err', `Terminate failed: ${e}`);
        } finally {
          awsVmBusy = false;
        }
      },
    };
  }

  async function runVmConfirm() {
    const c = vmConfirm;
    vmConfirm = null;
    if (c) await c.run();
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
      const a = await invoke<{ region: string; bucket: string; user_name: string; access_key: string; has_credentials: boolean }>('aws_get_config');
      if (a.region) awsRegion = a.region;
      awsBucket = a.bucket ?? '';
      awsUserName = a.user_name ?? '';
      awsAccessKey = a.access_key ?? '';
      awsHasCreds = a.has_credentials;
      // Saved credentials: re-verify silently so the account shows as Connected
      // automatically, without the user re-entering keys or clicking Connect.
      if (a.has_credentials) {
        try {
          awsAccount = await invoke<string>('aws_reverify');
        } catch {}
      }
    } catch {}
    try {
      awsVm = await invoke<{ provisioned: boolean; instance_id: string; host: string }>('aws_vm_status');
      if (awsVm.provisioned) await refreshVmState();
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
      // The port field is a text input, so its bound value is a string.
      // Normalise here rather than relying on the backend to coerce it.
      const port = Number(String(sshConfig.port).trim());
      // On the Cloud VM tab the connection target is the provisioned AWS VM
      // (stored separately), so the manual SSH fields are not required here.
      const isCloud = sshConfig.connection_type === 'cloud';
      if (!isCloud) {
        if (!sshConfig.host.trim() || !sshConfig.user.trim()) {
          showToast('err', 'SSH host and user are required.');
          busy = false;
          return;
        }
        if (!Number.isInteger(port) || port < 1 || port > 65535) {
          showToast('err', 'SSH port must be a number between 1 and 65535.');
          busy = false;
          return;
        }
      }
      const safePort = Number.isInteger(port) && port >= 1 && port <= 65535 ? port : 22;
      await invoke('save_ssh_config', { config: { ...sshConfig, port: safePort } });
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
        Linux server over SSH, or an auto-provisioned AWS VM.
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
        >AWS VM</button>
      </div>

      {#if sshConfig.connection_type === 'cloud'}
        <div class="aws-card">
          <div class="aws-head">
            <span class="aws-title"><span class="step-letter">ⓐ</span> AWS account</span>
            {#if awsAccount}
              <span class="aws-badge ok">Connected · {awsAccount}</span>
            {:else if awsHasCreds}
              <span class="aws-badge">Saved, click Connect to verify</span>
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
              <select class="ap-select" bind:value={awsRegion}>
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
          <p class="aws-hint">Pick the same region as your S3 bucket. Your VM is created there.</p>
          <div class="aws-actions">
            <button class="btn-outline small" disabled={awsBusy} onclick={awsConnect}>
              {awsBusy ? 'Connecting…' : 'Connect'}
            </button>
            <button class="btn-outline small" disabled={!awsUserName} onclick={awsSetupPermissions}>
              Set up AWS access
            </button>
          </div>
        </div>

        <div class="aws-card">
          <div class="aws-head">
            <span class="aws-title"><span class="step-letter">ⓑ</span> AWS VM (EC2)</span>
            {#if awsVm.provisioned}
              {#if awsVmState === 'running'}
                <span class="aws-badge ok">Running · {awsVm.host}</span>
              {:else if awsVmState === 'stopped'}
                <span class="aws-badge warn">Stopped</span>
              {:else if awsVmState === 'pending' || awsVmState === 'stopping'}
                <span class="aws-badge warn">{awsVmState}…</span>
              {:else}
                <span class="aws-badge">Provisioned</span>
              {/if}
            {/if}
          </div>
          {#if awsVm.provisioned}
            {#if awsVmState === 'stopped'}
              <p class="aws-hint">Stopped. Disk and tools kept, storage-only cost. Start to resume.</p>
              <div class="aws-actions">
                <button class="btn-primary small" disabled={awsVmBusy} onclick={awsStart}>
                  {awsVmBusy ? 'Starting…' : 'Start VM'}
                </button>
                <button class="btn-outline small danger" disabled={awsVmBusy} onclick={confirmTerminate}>
                  Terminate VM
                </button>
              </div>
            {:else}
              <p class="aws-hint">Billed hourly. Click Save and Register to use it. Stop = pause (keeps setup); Terminate = delete.</p>
              <div class="aws-actions">
                <button class="btn-outline small" disabled={awsVmBusy} onclick={confirmStop}>
                  {awsVmBusy ? 'Working…' : 'Stop VM'}
                </button>
                <button class="btn-outline small danger" disabled={awsVmBusy} onclick={confirmTerminate}>
                  Terminate VM
                </button>
                <button class="icon-btn" title="Refresh status" disabled={awsVmBusy} onclick={refreshVmState} aria-label="Refresh status">↻</button>
              </div>
            {/if}
          {:else}
            <p class="aws-hint">Creates an EC2 VM (Docker/Git/rclone auto-installed, ~2-4 min). Charged while running.</p>
            <div class="aws-actions">
              <button
                class="btn-primary small"
                disabled={awsVmBusy || (!awsAccount && !awsHasCreds)}
                onclick={awsProvision}
              >
                {awsVmBusy ? 'Provisioning… (2-4 min)' : 'Provision VM'}
              </button>
            </div>
          {/if}
        </div>

        <div class="aws-card">
          <div class="aws-head">
            <span class="aws-title"><span class="step-letter">ⓒ</span> S3 bucket</span>
            <button class="icon-btn" title="Refresh buckets" disabled={awsBusy} onclick={awsLoadBuckets} aria-label="Refresh buckets">↻</button>
          </div>
          <div class="aws-form">
            <label>
              <span>Bucket</span>
              <select
                class="ap-select"
                bind:value={awsBucket}
                onchange={(e) => awsSelectBucket((e.currentTarget as HTMLSelectElement).value)}
                disabled={awsBuckets.length === 0}
              >
                {#if awsBuckets.length === 0}
                  <option value={awsBucket}>{awsBucket || (awsAccount ? '(no buckets, create one in S3 then refresh)' : '(connect first to list buckets)')}</option>
                {:else}
                  <option value="">select</option>
                  {#each awsBuckets as b}
                    <option value={b}>{b}</option>
                  {/each}
                {/if}
              </select>
            </label>
          </div>
        </div>
      {:else}
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
      {/if}
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

  {#if awsSetupModal}
    <div class="overlay" role="dialog" aria-modal="true" aria-labelledby="aws-setup-title">
      <div class="aws-modal">
        <h2 id="aws-setup-title">Grant AWS access</h2>
        <ol>
          <li>Click <strong>Open CloudShell</strong> below. AWS CloudShell opens in your browser.</li>
          <li>
            When the shell prompt appears, <strong>paste</strong> this command (Ctrl+V, or
            Cmd+V on Mac) and press <strong>Enter</strong>:
            <div class="aws-cmd-row">
              <code class="aws-cmd">{awsSetupCmd}</code>
              <button class="cmd-copy" onclick={copySetupCmd}>{cmdCopied ? 'Copied' : 'Copy'}</button>
            </div>
          </li>
          <li>When it finishes, come back and click <strong>Provision VM</strong> in step <strong>ⓑ AWS VM</strong>.</li>
        </ol>
        <div class="aws-modal-actions">
          <button class="btn-outline small" onclick={() => (awsSetupModal = false)}>Cancel</button>
          <button class="btn-primary small" onclick={awsSetupOpenCloudShell}>Open CloudShell</button>
        </div>
      </div>
    </div>
  {/if}

  {#if vmConfirm}
    <div class="overlay" role="dialog" aria-modal="true" aria-labelledby="vm-confirm-title">
      <div class="aws-modal">
        <h2 id="vm-confirm-title">{vmConfirm.title}</h2>
        <ul class="vm-confirm-list">
          {#each vmConfirm.lines as line}
            <li>{line}</li>
          {/each}
        </ul>
        <div class="aws-modal-actions">
          <button class="btn-outline small" onclick={() => (vmConfirm = null)}>Cancel</button>
          <button
            class="btn-primary small"
            class:danger={vmConfirm.danger}
            onclick={runVmConfirm}
          >{vmConfirm.okLabel}</button>
        </div>
      </div>
    </div>
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
  /* App-styled dropdown: custom chevron, matches the text inputs. */
  .ap-select {
    box-sizing: border-box;
    width: 100%;
    appearance: none;
    -webkit-appearance: none;
    padding: 7px 32px 7px 10px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    font-size: 0.9rem;
    background-color: var(--bg-card);
    color: var(--text);
    font-family: inherit;
    background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%23888' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><polyline points='6 9 12 15 18 9'/></svg>");
    background-repeat: no-repeat;
    background-position: right 10px center;
    cursor: pointer;
  }
  .ap-select:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-light);
  }
  .ap-select:disabled { opacity: 0.6; cursor: default; }

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
    justify-content: space-between;
    gap: 10px;
    margin-bottom: 10px;
  }
  .aws-title {
    font-size: 0.9rem;
    font-weight: 600;
    color: var(--text);
  }
  .step-letter {
    color: var(--accent);
    font-weight: 700;
    margin-right: 4px;
  }
  .icon-btn {
    flex: 0 0 auto;
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-card);
    color: var(--text-muted);
    font-size: 1rem;
    line-height: 1;
    cursor: pointer;
  }
  .icon-btn:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
  }
  .icon-btn:disabled { opacity: 0.5; cursor: default; }
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
  .aws-badge.warn {
    color: #b45309;
    border-color: #f59e0b;
    background: rgba(245, 158, 11, 0.1);
  }
  .btn-outline.small.danger {
    color: var(--danger);
    border-color: var(--danger);
  }
  .btn-primary.small.danger {
    background: var(--danger);
    border-color: var(--danger);
  }
  .btn-primary.small.danger:hover {
    background: #b91c1c;
    border-color: #b91c1c;
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
  .aws-form input {
    box-sizing: border-box;
    width: 100%;
    padding: 7px 10px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    font-size: 0.9rem;
    background: var(--bg-card);
    color: var(--text);
    font-family: inherit;
  }
  .aws-form input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-light);
  }
  .aws-hint {
    margin: 8px 0 0;
    font-size: 0.8rem;
    color: var(--text-muted);
    line-height: 1.5;
  }
  .aws-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    flex-wrap: wrap;
    margin-top: 12px;
  }
  .btn-outline.small,
  .btn-primary.small {
    padding: 6px 14px;
    font-size: 0.82rem;
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

  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 60;
    padding: 24px;
  }
  .aws-modal {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 10px;
    width: 100%;
    max-width: 520px;
    padding: 22px 24px;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.25);
  }
  .aws-modal h2 {
    margin: 0 0 10px;
    font-size: 1.05rem;
  }
  .aws-modal ol {
    margin: 0 0 14px;
    padding-left: 20px;
    line-height: 1.6;
  }
  .vm-confirm-list {
    margin: 0 0 16px;
    padding-left: 20px;
    line-height: 1.6;
  }
  .vm-confirm-list li { margin: 0 0 4px; }
  .aws-cmd-row {
    display: flex;
    align-items: stretch;
    gap: 8px;
    margin: 8px 0 4px;
  }
  .aws-cmd {
    flex: 1;
    min-width: 0;
    background: var(--bg-soft);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 10px 12px;
    font-size: 0.78rem;
    white-space: pre-wrap;
    word-break: break-all;
    overflow-x: auto;
  }
  .cmd-copy {
    flex: 0 0 auto;
    padding: 0 14px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-card);
    color: var(--text);
    font-size: 0.8rem;
    font-weight: 500;
    cursor: pointer;
  }
  .cmd-copy:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .aws-modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
  }
</style>
