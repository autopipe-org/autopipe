<script lang="ts">
	import { CSS } from '$lib/styles.js';
	import { onMount } from 'svelte';

	let { data } = $props();

	let searchValue = $state(data.q);
	let currentPage = $state(1);
	let splashVisible = $state(true);
	let splashFading = $state(false);
	let appVisible = $state(false);

	const PAGE_SIZE = 10;

	const filteredPipelines = $derived(() => {
		const q = searchValue.toLowerCase();
		if (!q) return data.pipelines;
		return data.pipelines.filter(
			(p) =>
				p.name.toLowerCase().includes(q) ||
				p.description.toLowerCase().includes(q) ||
				p.tools.some((t) => t.toLowerCase().includes(q)) ||
				p.tags.some((t) => t.toLowerCase().includes(q))
		);
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

	function goToPage(page: number) {
		currentPage = page;
		document.getElementById('list-title')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
	}

	onMount(() => {
		setTimeout(() => {
			splashFading = true;
			setTimeout(() => {
				splashVisible = false;
				appVisible = true;
			}, 500);
		}, 1200);
	});
</script>

<svelte:head>
	<title>AutoPipe</title>
	{@html `<style>${CSS}</style>`}
</svelte:head>

{#if splashVisible}
	<div class="splash" class:splash-fade={splashFading}>
		<div class="splash-inner">
			<div class="splash-icon">
				<span class="dot"></span><span class="line"></span><span class="dot"></span><span
					class="line"
				></span><span class="dot"></span>
			</div>
			<div class="splash-title">AutoPipe</div>
			<div class="splash-sub">Bioinformatics Snakemake Pipeline Registry</div>
			<div class="splash-bar"><div class="splash-bar-fill"></div></div>
			<div class="splash-loading">Loading pipelines...</div>
		</div>
	</div>
{/if}

<div class:app-hidden={!appVisible}>
	<header>
		<a href="/" class="logo">AutoPipe</a>
		<nav style="margin-left:auto;display:flex;gap:16px">
			<a href="/" style="color:#fff;text-decoration:none;font-weight:600">Pipelines</a>
			<a href="/plugins" style="color:#ccc;text-decoration:none">Plugins</a>
		</nav>
	</header>
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
				{searchValue ? 'Search Results' : 'All Pipelines'}
				<span class="section-count">({filteredPipelines().length})</span>
			</h3>
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
							{#each p.tags as tag}
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
	</main>
</div>
