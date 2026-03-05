<script lang="ts">
	let { data } = $props();

	let searchValue = $state(data.q);
	let currentPage = $state(1);

	const PAGE_SIZE = 12;

	const filteredPlugins = $derived(() => {
		const q = searchValue.toLowerCase();
		if (!q) return data.plugins;
		return data.plugins.filter(
			(p) =>
				p.name.toLowerCase().includes(q) ||
				p.description.toLowerCase().includes(q) ||
				(p.extensions || []).some((e) => e.toLowerCase().includes(q))
		);
	});

	const totalPages = $derived(Math.max(1, Math.ceil(filteredPlugins().length / PAGE_SIZE)));
	const paginatedPlugins = $derived(() => {
		const page = Math.min(currentPage, totalPages);
		const start = (page - 1) * PAGE_SIZE;
		return filteredPlugins().slice(start, start + PAGE_SIZE);
	});

	function onSearchInput(e: Event) {
		searchValue = (e.target as HTMLInputElement).value;
		currentPage = 1;
	}

	function goToPage(page: number) {
		currentPage = page;
		document.getElementById('list-title')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
	}
</script>

<svelte:head>
	<title>Plugins - AutoPipe</title>
</svelte:head>

<main>
	<div class="plugin-guide">
		<p>Plugins extend the AutoPipe Results Viewer with custom file viewers. Install plugins in the app to preview additional file formats.</p>
		<a href="/plugins/guide" class="guide-link">Plugin Creation Guide &rarr;</a>
	</div>
	<div class="section">
		<h3 class="section-title">Search Plugins</h3>
		<div class="search">
			<input
				type="text"
				placeholder="Search by name, category, or tag..."
				value={searchValue}
				oninput={onSearchInput}
				autocomplete="off"
			/>
		</div>
	</div>
	<div class="section">
		<h3 class="section-title" id="list-title">
			{searchValue ? 'Search Results' : 'All Plugins'}
			<span class="section-count">({filteredPlugins().length})</span>
		</h3>
		<div class="plugin-grid">
			{#each paginatedPlugins() as p (p.plugin_id)}
				<a href="/plugins/{p.plugin_id}" class="plugin-card">
					<div class="plugin-card-header">
						<div class="plugin-card-icon">{p.name.charAt(0).toUpperCase()}</div>
						<div class="plugin-card-meta">
							<div class="plugin-card-name">{p.name}</div>
							<div class="plugin-card-version">v{p.version}</div>
						</div>
					</div>
					<div class="plugin-card-desc">{p.description}</div>
					{#if p.extensions && p.extensions.length > 0}
						<div class="plugin-card-exts">
							{#each p.extensions as ext}
								<span class="plugin-ext-tag">.{ext}</span>
							{/each}
						</div>
					{/if}
					<div class="plugin-card-footer">
						<span class="plugin-card-author">{p.author || 'unknown'}</span>
					</div>
				</a>
			{:else}
				<p class="empty">No plugins found.</p>
			{/each}
		</div>
		{#if totalPages > 1}
			<div class="pagination">
				{#if currentPage > 1}
					<button class="page-btn" onclick={() => goToPage(currentPage - 1)}>&laquo;</button>
				{/if}
				{#each Array.from({ length: totalPages }, (_, i) => i + 1) as page}
					<button
						class="page-btn"
						class:active={page === currentPage}
						onclick={() => goToPage(page)}
					>
						{page}
					</button>
				{/each}
				{#if currentPage < totalPages}
					<button class="page-btn" onclick={() => goToPage(currentPage + 1)}>&raquo;</button>
				{/if}
			</div>
		{/if}
	</div>
</main>
