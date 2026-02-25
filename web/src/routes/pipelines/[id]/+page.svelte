<script lang="ts">
	import { CSS } from '$lib/styles.js';

	let { data } = $props();
	const p = data.pipeline;

	let activeTab = $state(0);

	type FileTab = { name: string; content: string };
	const files: FileTab[] = [];
	if (p.snakefile) files.push({ name: 'Snakefile', content: p.snakefile });
	if (p.dockerfile) files.push({ name: 'Dockerfile', content: p.dockerfile });
	if (p.config_yaml) files.push({ name: 'config.yaml', content: p.config_yaml });
	if (p.metadata_json) {
		files.push({
			name: 'metadata.json',
			content: typeof p.metadata_json === 'string'
				? p.metadata_json
				: JSON.stringify(p.metadata_json, null, 2)
		});
	}
	if (p.readme) files.push({ name: 'README.md', content: p.readme });

	function switchTab(idx: number) {
		activeTab = idx;
	}
</script>

<svelte:head>
	<title>{p.name} - AutoPipe</title>
	{@html `<style>${CSS}</style>`}
</svelte:head>

<header>
	<a href="/" class="logo">AutoPipe</a>
</header>
<main>
	<a href="/" class="back-link">&larr; Back to list</a>
	<div class="detail-header">
		<div>
			<h2>{p.name}</h2>
			<p class="detail-desc">{p.description}</p>
		</div>
		<a href="/pipelines/{p.pipeline_id}/download" class="btn">Download ZIP</a>
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
			<span class="label">INPUT</span>
			<span class="value">{p.input_formats.join(', ')}</span>
		</div>
		<div class="detail-info-item">
			<span class="label">OUTPUT</span>
			<span class="value">{p.output_formats.join(', ')}</span>
		</div>
	</div>
	<div class="detail-tags">
		<span class="label">TOOLS</span>
		{#each p.tools as tool}
			<span class="tag tool">{tool}</span>
		{/each}
		<span class="label" style="margin-left:16px">TAGS</span>
		{#each p.tags as tag}
			<span class="tag">{tag}</span>
		{/each}
	</div>
	<div class="files-section">
		<div class="tab-bar">
			{#each files as file, idx}
				<button
					class="tab-btn"
					class:active={idx === activeTab}
					onclick={() => switchTab(idx)}
				>
					{file.name}
				</button>
			{/each}
		</div>
		<div class="tab-content">
			{#each files as file, idx}
				<div class="tab-panel" class:active={idx === activeTab}>
					<pre><code>{file.content}</code></pre>
				</div>
			{/each}
		</div>
	</div>
</main>
