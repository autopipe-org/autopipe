<script lang="ts">
  import Setup from '$lib/tabs/Setup.svelte';
  import SshTab from '$lib/tabs/Ssh.svelte';
  import GitHubTab from '$lib/tabs/GitHub.svelte';
  import StatusTab from '$lib/tabs/Status.svelte';

  type Tab = 'setup' | 'ssh' | 'github' | 'status';
  let active: Tab = $state('setup');

  const tabs: { id: Tab; label: string }[] = [
    { id: 'setup', label: 'Setup' },
    { id: 'ssh', label: 'SSH' },
    { id: 'github', label: 'GitHub' },
    { id: 'status', label: 'Status' },
  ];
</script>

<div class="app">
  <header class="tabbar">
    {#each tabs as t}
      <button
        class="tab"
        class:active={active === t.id}
        onclick={() => (active = t.id)}
      >
        {t.label}
      </button>
    {/each}
  </header>

  <main class="content">
    {#if active === 'setup'}<Setup />
    {:else if active === 'ssh'}<SshTab />
    {:else if active === 'github'}<GitHubTab />
    {:else if active === 'status'}<StatusTab />
    {/if}
  </main>
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .tabbar {
    display: flex;
    border-bottom: 1px solid #e5e7eb;
    background: #fff;
    padding: 0 12px;
  }
  .tab {
    background: none;
    border: none;
    padding: 12px 18px;
    font-size: 14px;
    font-weight: 500;
    color: #6b7280;
    cursor: pointer;
    border-bottom: 2px solid transparent;
  }
  .tab.active {
    color: #0f4c5c;
    border-bottom-color: #0f4c5c;
  }
  .tab:hover {
    color: #1f2937;
  }
  .content {
    flex: 1;
    overflow: auto;
    padding: 24px;
  }
</style>
