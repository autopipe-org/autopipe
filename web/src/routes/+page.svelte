<script lang="ts">
	let { data } = $props();

	let searchValue = $state(data.q);
	let currentPage = $state(1);
	let selectedTools = $state<Set<string>>(new Set());
	let selectedTags = $state<Set<string>>(new Set());
	let toolSearch = $state('');
	let tagSearch = $state('');

	const PAGE_SIZE = 10;

	// Extract all unique tools and tags with counts
	const allTools = $derived(() => {
		const counts = new Map<string, number>();
		for (const p of data.pipelines) {
			for (const t of p.tools) {
				counts.set(t, (counts.get(t) || 0) + 1);
			}
		}
		return [...counts.entries()].sort((a, b) => b[1] - a[1]);
	});

	const allTags = $derived(() => {
		const counts = new Map<string, number>();
		const toolSet = new Set(allTools().map(([name]) => name));
		for (const p of data.pipelines) {
			for (const t of p.tags) {
				if (!toolSet.has(t)) {
					counts.set(t, (counts.get(t) || 0) + 1);
				}
			}
		}
		return [...counts.entries()].sort((a, b) => b[1] - a[1]);
	});

	const visibleTools = $derived(() => {
		const q = toolSearch.toLowerCase();
		if (!q) return allTools();
		return allTools().filter(([name]) => name.toLowerCase().includes(q));
	});

	const visibleTags = $derived(() => {
		const q = tagSearch.toLowerCase();
		if (!q) return allTags();
		return allTags().filter(([name]) => name.toLowerCase().includes(q));
	});

	const filteredPipelines = $derived(() => {
		const q = searchValue.toLowerCase();
		return data.pipelines.filter((p) => {
			// Text search
			if (q && !(
				p.name.toLowerCase().includes(q) ||
				p.description.toLowerCase().includes(q) ||
				p.tools.some((t: string) => t.toLowerCase().includes(q)) ||
				p.tags.some((t: string) => t.toLowerCase().includes(q))
			)) return false;

			// Tool filter
			if (selectedTools.size > 0 && !p.tools.some((t: string) => selectedTools.has(t))) return false;

			// Tag filter
			if (selectedTags.size > 0 && !p.tags.some((t: string) => selectedTags.has(t))) return false;

			return true;
		});
	});

	const totalPages = $derived(Math.max(1, Math.ceil(filteredPipelines().length / PAGE_SIZE)));
	const paginatedPipelines = $derived(() => {
		const page = Math.min(currentPage, totalPages);
		const start = (page - 1) * PAGE_SIZE;
		return filteredPipelines().slice(start, start + PAGE_SIZE);
	});

	function onSearchInput(e: Event) {
		searchValue = (e.target as HTMLInputElement).value;
		currentPage = 1;
	}

	function toggleTool(tool: string) {
		const next = new Set(selectedTools);
		if (next.has(tool)) next.delete(tool); else next.add(tool);
		selectedTools = next;
		currentPage = 1;
	}

	function toggleTag(tag: string) {
		const next = new Set(selectedTags);
		if (next.has(tag)) next.delete(tag); else next.add(tag);
		selectedTags = next;
		currentPage = 1;
	}

	function clearFilters() {
		selectedTools = new Set();
		selectedTags = new Set();
		toolSearch = '';
		tagSearch = '';
		currentPage = 1;
	}

	function goToPage(page: number) {
		currentPage = page;
		document.getElementById('list-title')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
	}
</script>

<svelte:head>
	<title>Autopipe Hub</title>
</svelte:head>

<main>
	<div class="section">
		<h3 class="section-title">Search Pipelines</h3>
		<div class="search">
			<input
				type="text"
				placeholder="Search by name, tool, or tag..."
				value={searchValue}
				oninput={onSearchInput}
				autocomplete="off"
			/>
		</div>
	</div>
	<div class="section">
		<h3 class="section-title" id="list-title">
			{searchValue || selectedTools.size > 0 || selectedTags.size > 0 ? 'Filtered Results' : 'All Pipelines'}
			<span class="section-count">({filteredPipelines().length})</span>
		</h3>
		<div class="list-layout">
			<div class="filter-sidebar">
				<div class="filter-box">
					{#if selectedTools.size > 0 || selectedTags.size > 0}
						<button class="filter-clear" onclick={clearFilters}>Clear all filters</button>
					{/if}
					<div class="filter-title">TOOLS</div>
					<input
						class="filter-search"
						type="text"
						placeholder="Search tools..."
						bind:value={toolSearch}
						autocomplete="off"
					/>
					<div class="filter-group">
						{#each visibleTools() as [tool, count]}
							<label class="filter-item">
								<input type="checkbox" checked={selectedTools.has(tool)} onchange={() => toggleTool(tool)} />
								{tool}
								<span class="filter-count">{count}</span>
							</label>
						{/each}
						{#if visibleTools().length === 0}
							<span style="font-size:12px;color:#ccc">{toolSearch ? 'No matches' : 'No tools'}</span>
						{/if}
					</div>
					<div class="filter-title">TAGS</div>
					<input
						class="filter-search"
						type="text"
						placeholder="Search tags..."
						bind:value={tagSearch}
						autocomplete="off"
					/>
					<div class="filter-group">
						{#each visibleTags() as [tag, count]}
							<label class="filter-item">
								<input type="checkbox" checked={selectedTags.has(tag)} onchange={() => toggleTag(tag)} />
								{tag}
								<span class="filter-count">{count}</span>
							</label>
						{/each}
						{#if visibleTags().length === 0}
							<span style="font-size:12px;color:#ccc">{tagSearch ? 'No matches' : 'No tags'}</span>
						{/if}
					</div>
				</div>
			</div>
			<div class="list-content">
				<div class="grid">
					{#each paginatedPipelines() as p (p.pipeline_id)}
						<a href="/pipelines/{p.pipeline_id}" class="card">
							<div class="card-title">
								{p.name}
								<span class="card-version">v{p.version}</span>
							</div>
							<div class="card-desc">{p.description}</div>
							<div class="card-tags">
								{#each p.tools as tool}
									<span class="tag tool">{tool}</span>
								{/each}
								{#each p.tags.filter((t: string) => !p.tools.includes(t)) as tag}
									<span class="tag">{tag}</span>
								{/each}
							</div>
						</a>
					{:else}
						<p class="empty">No pipelines found.</p>
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
		</div>
	</div>
</main>
