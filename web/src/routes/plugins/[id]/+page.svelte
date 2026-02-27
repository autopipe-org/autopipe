<script lang="ts">
	let { data } = $props();
	const p = data.plugin;

	let showAll = $state(false);

	const displayedVersions = $derived(
		showAll ? data.versionChain : data.versionChain.slice(0, 3)
	);

	const metadataStr = typeof p.metadata_json === 'string'
		? p.metadata_json
		: JSON.stringify(p.metadata_json, null, 2);
</script>

<svelte:head>
	<title>{p.name} - Plugins - AutoPipe</title>
</svelte:head>

<main>
	<div class="back-link-wrap"><a href="/plugins" class="back-link">&larr; Back to list</a></div>
	<div class="detail-layout">
		<div class="detail-main">
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
					<button class="tab-btn active">metadata.json</button>
				</div>
				<div class="tab-content">
					<div class="tab-panel active">
						<pre><code>{metadataStr}</code></pre>
					</div>
				</div>
			</div>
		</div>
		<div class="detail-sidebar">
			<div class="sidebar-title">VERSIONS</div>
			<div class="version-timeline">
				<div class="version-line"></div>
				{#each displayedVersions as v (v.plugin_id)}
					<a href="/plugins/{v.plugin_id}" class="version-item">
						<div class="version-dot" class:current={v.plugin_id === p.plugin_id}></div>
						<div class="version-card" class:current={v.plugin_id === p.plugin_id}>
							<span class="version-ver">v{v.version}</span>
							{#if v.verified}<span class="version-badge">verified</span>{/if}
							<div class="version-meta">{v.created_at?.split('T')[0] || '—'} · {v.author || 'unknown'}</div>
						</div>
					</a>
				{/each}
				{#if data.versionChain.length > 3 && !showAll}
					<button class="version-more" onclick={() => showAll = true}>show more ({data.versionChain.length - 3} more)</button>
				{/if}
			</div>
		</div>
	</div>
</main>
