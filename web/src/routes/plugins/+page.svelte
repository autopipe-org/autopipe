<script lang="ts">
	import { CSS } from '$lib/styles.js';

	let { data } = $props();

	let searchValue = $state(data.q);
	let currentPage = $state(1);

	const PAGE_SIZE = 10;

	const filteredPlugins = $derived(() => {
		const q = searchValue.toLowerCase();
		if (!q) return data.plugins;
		return data.plugins.filter(
			(p) =>
				p.name.toLowerCase().includes(q) ||
				p.description.toLowerCase().includes(q) ||
				p.category.toLowerCase().includes(q) ||
				p.tags.some((t) => t.toLowerCase().includes(q))
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
	{@html `<style>${CSS}</style>`}
</svelte:head>

<header>
	<div class="header-top">
		<a href="/" class="logo"><img src="/logo.png" alt="" class="logo-icon">AutoPipe</a>
		<span class="header-sub">Bioinformatics Snakemake Pipeline Registry</span>
	</div>
	<nav class="header-tabs">
		<a href="/" class="header-tab">Pipelines</a>
		<a href="/plugins" class="header-tab active">Plugins</a>
	</nav>
</header>
<main>
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
		<div class="grid">
			{#each paginatedPlugins() as p (p.plugin_id)}
				<a href="/plugins/{p.plugin_id}" class="card">
					<div class="card-title">
						{p.name}
						<span class="card-version">v{p.version}</span>
					</div>
					<div class="card-desc">{p.description}</div>
					<div class="card-tags">
						{#if p.category}
							<span class="tag tool">{p.category}</span>
						{/if}
						{#each p.tags as tag}
							<span class="tag">{tag}</span>
						{/each}
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
