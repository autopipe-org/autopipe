<script lang="ts">
  type SshConfig = {
    host: string;
    port: number;
    user: string;
    password: string;
    repo_path: string;
  };

  let { config = $bindable() }: { config: SshConfig } = $props();
</script>

<div class="form">
  <label>
    <span>Host</span>
    <input bind:value={config.host} placeholder="e.g. 127.0.0.1 or 10.0.0.5" />
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
    <span>Repo path</span>
    <input bind:value={config.repo_path} placeholder="/home/<user>/autopipe" />
  </label>
</div>

<style>
  .form {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 10px 14px;
    align-items: center;
  }
  label {
    display: contents;
  }
  label > span {
    font-size: 0.85rem;
    color: #4b5563;
    font-weight: 500;
  }
  input {
    padding: 7px 10px;
    border: 1px solid #d1d5db;
    border-radius: 5px;
    font-size: 0.9rem;
    background: #f9fafb;
  }
  input:focus {
    outline: none;
    border-color: #0f4c5c;
    background: #fff;
    box-shadow: 0 0 0 3px rgba(15, 76, 92, 0.1);
  }
</style>
