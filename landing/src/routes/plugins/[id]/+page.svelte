<script lang="ts">
	import { marked } from 'marked';

	let { data } = $props();
	const p = data.plugin;

	// Version history from JSONB (previous versions, newest first)
	const versionHistory = $derived(
		[...(p.version_history || [])].reverse()
	);

	// Related plugins (forks) — exclude self
	const relatedPlugins = $derived(
		data.versionChain.filter((v: { plugin_id: number }) => v.plugin_id !== p.plugin_id)
	);

	// Sanitize: escape raw HTML in markdown to prevent XSS
	function sanitizeHtml(html: string): string {
		return html
			.replace(/<script\b[^<]*(?:(?!<\/script>)<[^<]*)*<\/script>/gi, '')
			.replace(/<iframe\b[^>]*>.*?<\/iframe>/gi, '')
			.replace(/<object\b[^>]*>.*?<\/object>/gi, '')
			.replace(/<embed\b[^>]*\/?>/gi, '')
			.replace(/<link\b[^>]*\/?>/gi, '')
			.replace(/\bon\w+\s*=\s*["'][^"']*["']/gi, '')
			.replace(/\bon\w+\s*=\s*\S+/gi, '')
			.replace(/javascript\s*:/gi, '');
	}

	const readmeHtml = p.readme ? sanitizeHtml(marked(p.readme) as string) : '';

	let showAllVersions = $state(false);
	const MAX_VISIBLE = 3;
	const displayedVersions = $derived(
		showAllVersions ? versionHistory : versionHistory.slice(0, MAX_VISIBLE)
	);
	const hiddenCount = $derived(versionHistory.length - MAX_VISIBLE);
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
				{#if p.extensions && p.extensions.length > 0}
					<div class="detail-info-item">
						<span class="label">EXTENSIONS</span>
						<span class="value">{p.extensions.map(e => '.' + e).join(', ')}</span>
					</div>
				{/if}
			</div>
			{#if readmeHtml}
				<div class="readme-section">
					<div class="readme-content">
						{@html readmeHtml}
					</div>
				</div>
			{:else}
				<div class="readme-section">
					<p class="readme-empty">No README available.</p>
				</div>
			{/if}
		</div>
		<div class="detail-sidebar">
			<div class="sidebar-title">VERSION HISTORY</div>
			<div class="version-timeline">
				<div class="version-line"></div>
				<!-- Current version -->
				<div class="version-item">
					<div class="version-dot current"></div>
					<div class="version-card current">
						<span class="version-ver">v{p.version}</span>
						<span class="version-badge">current</span>
						<div class="version-meta">{p.updated_at?.split('T')[0] || p.created_at?.split('T')[0] || '—'}</div>
					</div>
				</div>
				<!-- Previous versions -->
				{#each displayedVersions as vh, i (i)}
					<div class="version-item">
						<div class="version-dot"></div>
						<div class="version-card">
							<span class="version-ver">v{vh.version}</span>
							<div class="version-meta">{vh.updated_at?.split('T')[0] || '—'}</div>
						</div>
					</div>
				{/each}
				{#if !showAllVersions && hiddenCount > 0}
					<button class="version-view-more" onclick={() => showAllVersions = true}>
						show more ({hiddenCount} more)
					</button>
				{:else if showAllVersions && versionHistory.length > MAX_VISIBLE}
					<button class="version-view-more" onclick={() => showAllVersions = false}>
						show less
					</button>
				{/if}
			</div>
			{#if relatedPlugins.length > 0}
				<div class="sidebar-title" style="margin-top:24px">RELATED PLUGINS</div>
				<div class="version-timeline">
					<div class="version-line"></div>
					{#each relatedPlugins as v (v.plugin_id)}
						<a href="/plugins/{v.plugin_id}" class="version-item">
							<div class="version-dot"></div>
							<div class="version-card">
								<span class="version-ver">v{v.version}</span>
								<div class="version-meta">{v.created_at?.split('T')[0] || '—'} · {v.author || 'unknown'}</div>
							</div>
						</a>
					{/each}
				</div>
			{/if}
		</div>
	</div>
</main>
