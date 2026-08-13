<script lang="ts">
  type SshConfig = {
    host: string;
    port: number;
    user: string;
    auth_method: string; // 'password' | 'key'
    password: string;
    key_path: string;
    repo_path: string;
    connection_type: string;
    cloud_provider: string;
  };

  let { config = $bindable() }: { config: SshConfig } = $props();
</script>

<div class="form">
  <label>
    <span>Host</span>
    <input bind:value={config.host} placeholder="e.g. 127.0.0.1 or a cloud VM public IP" />
  </label>
  <label>
    <span>Port</span>
    <input
      type="text"
      inputmode="numeric"
      pattern="[0-9]*"
      bind:value={config.port}
    />
  </label>
  <label>
    <span>User</span>
    <input bind:value={config.user} placeholder="e.g. ubuntu, ec2-user" />
  </label>
  <label>
    <span>Auth</span>
    <div class="auth-choice">
      <label class="radio">
        <input type="radio" bind:group={config.auth_method} value="password" />
        Password
      </label>
      <label class="radio">
        <input type="radio" bind:group={config.auth_method} value="key" />
        SSH key
      </label>
    </div>
  </label>
  {#if config.auth_method === 'key'}
    <label>
      <span>Key file</span>
      <input bind:value={config.key_path} placeholder="e.g. ~/.ssh/my-vm.pem" />
    </label>
  {:else}
    <label>
      <span>Password</span>
      <input type="password" bind:value={config.password} />
    </label>
  {/if}
  <label>
    <span>Repo path</span>
    <input bind:value={config.repo_path} placeholder="/home/<user>/autopipe" />
  </label>
</div>

<style>
  .form {
    display: grid;
    grid-template-columns: 100px 1fr;
    gap: 10px 14px;
    align-items: center;
    margin-left: 34px;
  }
  label { display: contents; }
  label > span {
    font-size: 0.85rem;
    color: var(--text-muted);
    font-weight: 500;
  }
  input {
    padding: 7px 10px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    font-size: 0.9rem;
    background: var(--bg-card);
    color: var(--text);
    font-family: inherit;
  }
  input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-light);
  }
  .auth-choice {
    display: flex;
    gap: 16px;
    align-items: center;
  }
  .radio {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 0.88rem;
    color: var(--text);
    cursor: pointer;
  }
  .radio > input {
    width: auto;
    padding: 0;
    margin: 0;
    accent-color: var(--accent);
  }
</style>
