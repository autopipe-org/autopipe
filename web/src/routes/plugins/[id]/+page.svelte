<script lang="ts">
	import { CSS } from '$lib/styles.js';

	let { data } = $props();
	const p = data.plugin;

	const metadataStr = typeof p.metadata_json === 'string'
		? p.metadata_json
		: JSON.stringify(p.metadata_json, null, 2);
</script>

<svelte:head>
	<title>{p.name} - Plugins - AutoPipe</title>
	{@html `<style>${CSS}</style>`}
</svelte:head>

<header>
	<a href="/" class="logo">AutoPipe</a>
	<nav style="margin-left:auto;display:flex;gap:16px">
		<a href="/" style="color:#ccc;text-decoration:none">Pipelines</a>
		<a href="/plugins" style="color:#fff;text-decoration:none;font-weight:600">Plugins</a>
	</nav>
</header>
<main>
	<a href="/plugins" class="back-link">&larr; Back to plugins</a>
	<div class="detail-header">
		<div>
			<h2>{p.name}</h2>
			<p class="detail-desc">{p.description}</p>
		</div>
		<div style="display:flex;gap:8px">
			<a href={p.github_url} target="_blank" rel="noopener" class="btn" style="background:#24292e">GitHub</a>
		</div>
	</div>
	<div class="detail-info">
		<div class="detail-info-item">
			<span class="label">VERSION</span>
			<span class="value">{p.version}</span>
		</div>
		<div class="detail-info-item">
			<span class="label">AUTHOR</span>
			<span class="value">{p.author || 'unknown'}</span>
		</div>
		<div class="detail-info-item">
			<span class="label">CATEGORY</span>
			<span class="value">{p.category || '—'}</span>
		</div>
	</div>
	<div class="detail-tags">
		<span class="label">TAGS</span>
		{#each p.tags as tag}
			<span class="tag">{tag}</span>
		{/each}
		{#if p.tags.length === 0}
			<span style="color:#888">—</span>
		{/if}
	</div>
	<div class="files-section">
		<div class="tab-bar">
			<button class="tab-btn active">metadata.json</button>
		</div>
		<div class="tab-content">
			<div class="tab-panel active">
				<pre><code>{metadataStr}</code></pre>
			</div>
		</div>
	</div>
</main>
