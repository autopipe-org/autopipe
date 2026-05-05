<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  type SshConfig = {
    host: string;
    port: number;
    user: string;
    password: string;
    repo_path: string;
  };

  let config = $state<SshConfig>({
    host: '',
    port: 22,
    user: '',
    password: '',
    repo_path: '',
  });
  let busy = $state(false);
  let message = $state('');

  onMount(async () => {
    try {
      const loaded = await invoke<SshConfig>('get_ssh_config');
      config = loaded;
    } catch (e) {
      message = `Failed to load: ${e}`;
    }
  });

  async function save() {
    busy = true;
    message = '';
    try {
      await invoke('save_ssh_config', { config });
      message = 'Saved.';
    } catch (e) {
      message = `Error: ${e}`;
    } finally {
      busy = false;
    }
  }
</script>

<section>
  <h2>SSH</h2>
  <p>Configure the remote server where pipelines will be executed.</p>

  <div class="form">
    <label>
      <span>Host</span>
      <input bind:value={config.host} placeholder="127.0.0.1" />
    </label>
    <label>
      <span>Port</span>
      <input type="number" bind:value={config.port} />
    </label>
    <label>
      <span>User</span>
      <input bind:value={config.user} />
    </label>
    <label>
      <span>Password</span>
      <input type="password" bind:value={config.password} />
    </label>
    <label>
      <span>Remote Repo Path</span>
      <input bind:value={config.repo_path} placeholder="/home/user/autopipe" />
    </label>
  </div>

  <button class="primary" disabled={busy} onclick={save}>Save</button>

  {#if message}
    <p class="msg">{message}</p>
  {/if}
</section>

<style>
  h2 { margin: 0 0 12px; font-size: 1.25rem; }
  p { color: #6b7280; margin: 0 0 20px; }
  .form {
    display: grid;
    grid-template-columns: 140px 1fr;
    gap: 12px 16px;
    align-items: center;
    max-width: 600px;
    margin-bottom: 20px;
  }
  label {
    display: contents;
  }
  label > span {
    font-size: 0.9rem;
    color: #374151;
  }
  input {
    padding: 8px 12px;
    border: 1px solid #d1d5db;
    border-radius: 6px;
    font-size: 0.9rem;
  }
  input:focus {
    outline: none;
    border-color: #0f4c5c;
    box-shadow: 0 0 0 3px rgba(15, 76, 92, 0.1);
  }
  button.primary {
    padding: 10px 18px;
    background: #0f4c5c;
    color: #fff;
    border: none;
    border-radius: 6px;
    font-weight: 500;
    cursor: pointer;
  }
  button:disabled { opacity: 0.6; cursor: not-allowed; }
  .msg { margin-top: 16px; color: #047857; font-size: 0.9rem; }
</style>
