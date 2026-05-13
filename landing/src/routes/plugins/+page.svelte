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
		<p>Plugins extend the AutoPipe Results Viewer with custom file viewers.</p>
		<p>
			This page is a catalog for browsing available plugins —
			to install, update, or uninstall them, open the AutoPipe desktop app
			and click the Plugins button.
		</p>
		<div class="plugin-guide-links">
			<a href="/getting-started" class="guide-link">Get AutoPipe &rarr;</a>
			<a href="/plugins/guide" class="guide-link">Plugin Development Guide &rarr;</a>
		</div>
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
					<div class="plugin-card-icon">
						<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0f4c5c" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
							<path d="M9 2v6"/>
							<path d="M15 2v6"/>
							<path d="M12 17v5"/>
							<path d="M5 8h14"/>
							<path d="M6 11V8h12v3a6 6 0 0 1-12 0Z"/>
						</svg>
					</div>
					<h3 class="plugin-card-name">{p.name}</h3>
					<p class="plugin-card-desc">{p.description}</p>
					{#if p.extensions && p.extensions.length > 0}
						<div class="plugin-card-exts">
							{#each p.extensions as ext}
								<span class="plugin-ext-tag">.{ext}</span>
							{/each}
						</div>
					{/if}
					<div class="plugin-card-footer">
						<span class="plugin-card-author">{p.author || 'unknown'}</span>
						<span class="plugin-card-version">v{p.version}</span>
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
