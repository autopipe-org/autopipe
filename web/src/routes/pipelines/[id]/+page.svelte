<script lang="ts">
	import { CSS } from '$lib/styles.js';

	let { data } = $props();
	const p = data.pipeline;
	const f = data.files;

	let activeTab = $state(0);
	let showAll = $state(false);

	const displayedVersions = $derived(
		showAll ? data.versionChain : data.versionChain.slice(0, 3)
	);

	type FileTab = { name: string; content: string };
	const files: FileTab[] = [];
	if (f.snakefile) files.push({ name: 'Snakefile', content: f.snakefile });
	if (f.dockerfile) files.push({ name: 'Dockerfile', content: f.dockerfile });
	if (f.config_yaml) files.push({ name: 'config.yaml', content: f.config_yaml });
	if (f.metadata_json) {
		files.push({
			name: 'metadata.json',
			content: f.metadata_json
		});
	}
	if (f.readme) files.push({ name: 'README.md', content: f.readme });

	function switchTab(idx: number) {
		activeTab = idx;
	}
</script>

<svelte:head>
	<title>{p.name} - AutoPipe</title>
	{@html `<style>${CSS}</style>`}
</svelte:head>

<header>
	<div class="header-top">
		<a href="/" class="logo"><img src="/logo.png" alt="" class="logo-icon">AutoPipe</a>
		<span class="header-sub">Bioinformatics Pipeline Registry</span>
	</div>
	<nav class="header-tabs">
		<a href="/" class="header-tab active">Pipelines</a>
		<a href="/plugins" class="header-tab">Plugins</a>
	</nav>
</header>
<main>
	<a href="/" class="back-link">&larr; Back to list</a>
	<div class="detail-layout">
		<div class="detail-main">
			<div class="detail-header">
				<div>
					<h2>{p.name}</h2>
					<p class="detail-desc">{p.description}</p>
					{#if data.basedOn}
						<p class="based-on">Based on: <a href="/pipelines/{data.basedOn.pipeline_id}">{data.basedOn.name} v{data.basedOn.version}</a> by {data.basedOn.author}</p>
					{/if}
				</div>
				<div style="display:flex;gap:8px">
					<a href={p.github_url} target="_blank" rel="noopener" class="btn" style="background:#24292e">GitHub</a>
					<a href="/pipelines/{p.pipeline_id}/download" class="btn">Download ZIP</a>
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
					<span class="label">INPUT</span>
					<span class="value">
						{#each p.input_formats as fmt, i}
							<span class="tag">{fmt}</span>
						{/each}
						{#if p.input_formats.length === 0}—{/if}
					</span>
				</div>
				<div class="detail-info-item">
					<span class="label">OUTPUT</span>
					<span class="value">
						{#each p.output_formats as fmt}
							<span class="tag">{fmt}</span>
						{/each}
						{#if p.output_formats.length === 0}—{/if}
					</span>
				</div>
			</div>
			<div class="detail-tags">
				<div class="tag-row">
					<span class="label">TOOLS</span>
					<div class="tag-list">
						{#each p.tools as tool}
							<span class="tag tool">{tool}</span>
						{/each}
						{#if p.tools.length === 0}<span class="tag-empty">—</span>{/if}
					</div>
				</div>
				<div class="tag-row">
					<span class="label">TAGS</span>
					<div class="tag-list">
						{#each p.tags as tag}
							<span class="tag">{tag}</span>
						{/each}
						{#if p.tags.length === 0}<span class="tag-empty">—</span>{/if}
					</div>
				</div>
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
		</div>
		<div class="detail-sidebar">
			<div class="sidebar-title">VERSIONS</div>
			<div class="version-timeline">
				<div class="version-line"></div>
				{#each displayedVersions as v, i (v.pipeline_id)}
					{#if i === 0}
						<div class="version-item">
							<div class="version-dot current"></div>
							<div class="version-card current">
								<span class="version-ver">v{v.version}</span>
								<span class="version-badge">latest</span>
								<div class="version-meta">{v.created_at?.split('T')[0] || '—'} · {v.author || 'unknown'}</div>
							</div>
						</div>
					{:else}
						<a href="/pipelines/{v.pipeline_id}/download?tag={encodeURIComponent(v.name + '/v' + v.version)}" class="version-item">
							<div class="version-dot"></div>
							<div class="version-card">
								<span class="version-ver">v{v.version}</span>
								<span class="version-badge download">&darr;</span>
								<div class="version-meta">{v.created_at?.split('T')[0] || '—'} · {v.author || 'unknown'}</div>
							</div>
						</a>
					{/if}
				{/each}
				{#if data.versionChain.length > 3 && !showAll}
					<button class="version-more" onclick={() => showAll = true}>show more ({data.versionChain.length - 3} more)</button>
				{/if}
			</div>
		</div>
	</div>
</main>
