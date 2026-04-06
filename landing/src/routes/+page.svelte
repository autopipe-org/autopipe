<script lang="ts">
	import { env } from '$env/dynamic/public';

	const hubUrl = env.PUBLIC_HUB_URL;
	let menuOpen = $state(false);

	const features = [
		{
			icon: `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0f4c5c" stroke-width="2"><path d="M12 2L2 7l10 5 10-5-10-5z"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/></svg>`,
			title: 'AI Pipeline Generation',
			desc: 'Describe your analysis in natural language. AI generates a complete Snakemake pipeline with Dockerfile, config, and metadata.'
		},
		{
			icon: `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0f4c5c" stroke-width="2"><rect x="2" y="3" width="20" height="18" rx="2"/><path d="M9 3v18"/><path d="M14 9l3 3-3 3"/></svg>`,
			title: 'Container Execution',
			desc: 'Run pipelines in isolated containers on remote SSH servers. Fully reproducible environments with automated build and execution.'
		},
		{
			icon: `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0f4c5c" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M2 12h20"/><path d="M12 2a15 15 0 010 20 15 15 0 010-20z"/></svg>`,
			title: 'Registry Integration',
			desc: 'Search AutoPipeHub and WorkflowHub simultaneously. Import existing workflows and extend them with your own analysis steps.'
		},
		{
			icon: `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0f4c5c" stroke-width="2"><path d="M2 3h6a4 4 0 014 4v14a3 3 0 00-3-3H2z"/><path d="M22 3h-6a4 4 0 00-4 4v14a3 3 0 013-3h7z"/></svg>`,
			title: 'Interactive Viewer',
			desc: 'Browse results directly in the browser. Built-in support for IGV genomics, images, tables, PDFs, and HDF5 with extensible plugins.'
		}
	];

	const viewerPlugins = [
		{
			icon: `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0f4c5c" stroke-width="2"><path d="M3 3v18h18"/><path d="M7 16l4-8 4 4 4-6"/></svg>`,
			title: 'IGV Genomics',
			desc: 'Visualize BAM, BED, GFF, and more with IGV.js. Supports reference genome selection for interactive genome browsing.'
		},
		{
			icon: `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0f4c5c" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg>`,
			title: 'Image Viewer',
			desc: 'Display PNG, JPG, SVG, and TIFF scientific images. Zoom, pan, and compare experimental result images side by side.'
		},
		{
			icon: `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0f4c5c" stroke-width="2"><path d="M12 3v18"/><path d="M3 12h18"/><rect x="3" y="3" width="18" height="18" rx="1"/><path d="M3 8h18"/><path d="M3 16h18"/><path d="M8 3v18"/><path d="M16 3v18"/></svg>`,
			title: 'Data Table',
			desc: 'Render CSV and TSV files as sortable, filterable tables. Quickly inspect pipeline output metrics and summary statistics.'
		},
		{
			icon: `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0f4c5c" stroke-width="2"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg>`,
			title: 'PDF & HDF5',
			desc: 'View PDF documents and browse HDF5 scientific data files. Navigate hierarchical datasets directly in the result viewer.'
		}
	];
</script>

<svelte:head>
	<title>Autopipe</title>
	<meta name="description" content="AI-powered bioinformatics pipeline generation, execution, and sharing platform." />
</svelte:head>

<!-- Header -->
<header>
	<nav>
		<a href="/" class="logo">
			<img src="/logo.png" alt="Autopipe" />
			<span>Autopipe</span>
		</a>
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<button class="hamburger" class:open={menuOpen} onclick={() => menuOpen = !menuOpen}>
			<span></span><span></span><span></span>
		</button>
		<div class="nav-links" class:open={menuOpen}>
			<a href={hubUrl} target="_blank" rel="noopener" onclick={() => menuOpen = false}>Hub <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="display:inline;vertical-align:middle;margin-left:2px"><path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg></a>
			<a href="/plugins" onclick={() => menuOpen = false}>Plugins</a>
			<a href="/getting-started" onclick={() => menuOpen = false}>Getting Started</a>
		</div>
	</nav>
</header>

<!-- Hero -->
<section class="hero">
	<div class="hero-content">
		<div class="hero-text">
			<h1>End-to-End Pipeline Automation</h1>
			<p>Generate, execute, visualize, and share<br/>reproducible containerized pipelines with AI.</p>
			<a href="/getting-started" class="btn-primary">Get Started</a>
		</div>
		<div class="hero-visual">
			<img src="/autopipe_landing.webp" alt="Autopipe architecture overview" class="hero-img" />
		</div>
	</div>
</section>

<!-- Features -->
<section class="features" id="about">
	<h2>Why Autopipe?</h2>
	<div class="features-grid">
		{#each features as f}
			<div class="feature-card">
				<div class="feature-icon">{@html f.icon}</div>
				<h3>{f.title}</h3>
				<p>{f.desc}</p>
			</div>
		{/each}
	</div>
</section>

<!-- Viewer Plugins -->
<section class="features viewer-section">
	<div class="section-header">
		<h2>Viewer Plugins</h2>
		<p class="section-desc">Extend the built-in result viewer with plugins for specialized file formats.</p>
	</div>
	<div class="features-grid">
		{#each viewerPlugins as p}
			<div class="feature-card">
				<div class="feature-icon">{@html p.icon}</div>
				<h3>{p.title}</h3>
				<p>{p.desc}</p>
			</div>
		{/each}
	</div>
	<div class="section-cta">
		<a href="/plugins" class="plugins-link">Browse all plugins &rarr;</a>
	</div>
</section>

<!-- Download CTA -->
<section class="download">
	<h2>Get Autopipe Desktop</h2>
	<p>Download the desktop app to create, execute, and manage bioinformatics pipelines with AI.</p>
	<div class="download-buttons">
		<a href="/getting-started" class="btn-primary">Get Started</a>
		<a href={hubUrl} target="_blank" rel="noopener" class="btn-secondary">Browse AutoPipeHub</a>
	</div>
</section>

<!-- Footer -->
<footer>
	<div class="footer-content">
		<a href="/" class="footer-logo">
			<img src="/logo.png" alt="Autopipe" />
			<span>Autopipe</span>
		</a>
		<span class="footer-copy">&copy; 2026 Autopipe</span>
	</div>
</footer>

<style>
	:global(*) {
		margin: 0;
		padding: 0;
		box-sizing: border-box;
	}
	:global(body) {
		font-family: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
		color: #1a2332;
		background: #fff;
		line-height: 1.6;
	}

	/* Header */
	header {
		position: sticky;
		top: 0;
		background: #fff;
		border-bottom: 1px solid #e5e7eb;
		z-index: 100;
	}
	nav {
		max-width: 1200px;
		margin: 0 auto;
		padding: 16px 24px;
		display: flex;
		align-items: center;
		justify-content: space-between;
	}
	.logo {
		display: flex;
		align-items: center;
		gap: 10px;
		text-decoration: none;
		color: #1a2332;
		font-weight: 700;
		font-size: 1.25rem;
	}
	.logo img {
		height: 32px;
		width: auto;
	}
	.nav-links {
		display: flex;
		gap: 32px;
	}
	.nav-links a {
		text-decoration: none;
		color: #4b5563;
		font-weight: 500;
		font-size: 0.95rem;
		transition: color 0.2s;
	}
	.nav-links a:hover {
		color: #1a2332;
	}

	/* Hamburger */
	.hamburger {
		display: none;
		background: none;
		border: none;
		cursor: pointer;
		padding: 4px;
		flex-direction: column;
		gap: 5px;
	}
	.hamburger span {
		display: block;
		width: 24px;
		height: 2px;
		background: #1a2332;
		transition: transform 0.3s, opacity 0.3s;
	}
	.hamburger.open span:nth-child(1) {
		transform: translateY(7px) rotate(45deg);
	}
	.hamburger.open span:nth-child(2) {
		opacity: 0;
	}
	.hamburger.open span:nth-child(3) {
		transform: translateY(-7px) rotate(-45deg);
	}

	/* Hero */
	.hero {
		background: linear-gradient(135deg, #0f4c5c 0%, #1a6b7a 40%, #2d8a99 100%);
		color: #fff;
		overflow: hidden;
	}
	.hero-content {
		max-width: 1200px;
		margin: 0 auto;
		padding: 80px 24px;
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 60px;
		align-items: center;
	}
	.hero h1 {
		font-size: 3rem;
		font-weight: 700;
		line-height: 1.15;
		margin-bottom: 20px;
	}
	.hero p {
		font-size: 1.1rem;
		opacity: 0.9;
		margin-bottom: 32px;
		max-width: 480px;
	}
	.btn-primary {
		display: inline-block;
		padding: 14px 32px;
		background: #10b981;
		color: #fff;
		text-decoration: none;
		border-radius: 8px;
		font-weight: 600;
		font-size: 1rem;
		transition: background 0.2s, transform 0.2s;
	}
	.btn-primary:hover {
		background: #059669;
		transform: translateY(-1px);
	}
	.btn-secondary {
		display: inline-block;
		padding: 14px 32px;
		background: transparent;
		color: #0f4c5c;
		text-decoration: none;
		border-radius: 8px;
		font-weight: 600;
		font-size: 1rem;
		border: 2px solid #0f4c5c;
		transition: background 0.2s;
	}
	.btn-secondary:hover {
		background: #f0fdf4;
	}

	/* Hero image */
	.hero-visual {
		display: flex;
		justify-content: center;
		align-items: center;
	}
	.hero-img {
		width: 100%;
		max-width: 560px;
		height: auto;
		border-radius: 12px;
	}

	/* Features */
	.features {
		max-width: 1200px;
		margin: 0 auto;
		padding: 80px 24px;
	}
	.features h2 {
		font-size: 1.75rem;
		font-weight: 700;
		margin-bottom: 48px;
	}
	.features-grid {
		display: grid;
		grid-template-columns: repeat(4, 1fr);
		gap: 24px;
	}
	.feature-card {
		padding: 24px;
	}
	.feature-icon {
		margin-bottom: 14px;
		line-height: 1;
	}
	.feature-card h3 {
		font-size: 1rem;
		font-weight: 600;
		margin-bottom: 8px;
	}
	.feature-card p {
		font-size: 0.875rem;
		color: #6b7280;
		line-height: 1.6;
	}

	/* Viewer Plugins section */
	.viewer-section {
		padding-top: 0;
	}
	.section-header {
		margin-bottom: 48px;
	}
	.section-header h2 {
		margin-bottom: 8px;
	}
	.section-desc {
		color: #6b7280;
		font-size: 1rem;
	}
	.section-cta {
		margin-top: 24px;
	}
	.plugins-link {
		color: #0f4c5c;
		font-weight: 600;
		text-decoration: none;
		font-size: 0.95rem;
	}
	.plugins-link:hover {
		text-decoration: underline;
	}

	/* Download CTA */
	.download {
		text-align: center;
		padding: 80px 24px;
		background: #f9fafb;
	}
	.download h2 {
		font-size: 2rem;
		font-weight: 700;
		margin-bottom: 16px;
	}
	.download p {
		color: #6b7280;
		margin-bottom: 32px;
		max-width: 560px;
		margin-left: auto;
		margin-right: auto;
	}
	.download-buttons {
		display: flex;
		gap: 16px;
		justify-content: center;
	}

	/* Footer */
	footer {
		border-top: 1px solid #e5e7eb;
		padding: 24px;
	}
	.footer-content {
		max-width: 1200px;
		margin: 0 auto;
		display: flex;
		align-items: center;
		justify-content: space-between;
	}
	.footer-logo {
		display: flex;
		align-items: center;
		gap: 8px;
		font-weight: 700;
		text-decoration: none;
		color: #1a2332;
	}
	.footer-logo img {
		height: 20px;
	}
	.footer-copy {
		color: #9ca3af;
		font-size: 0.8rem;
	}

	/* Responsive */
	@media (max-width: 768px) {
		.hamburger {
			display: flex;
		}
		.nav-links {
			display: none;
			position: absolute;
			top: 100%;
			left: 0;
			right: 0;
			background: #fff;
			flex-direction: column;
			padding: 16px 24px;
			gap: 16px;
			border-bottom: 1px solid #e5e7eb;
			box-shadow: 0 4px 12px rgba(0,0,0,0.08);
		}
		.nav-links.open {
			display: flex;
		}
		nav {
			position: relative;
		}
		.hero-content {
			grid-template-columns: 1fr;
			padding: 48px 24px;
		}
		.hero h1 {
			font-size: 2rem;
		}
		.hero-img {
			max-width: 100%;
		}
		.features-grid {
			grid-template-columns: 1fr 1fr;
		}
		.download-buttons {
			flex-direction: column;
			align-items: center;
		}
		.footer-content {
			flex-direction: column;
			gap: 8px;
		}
	}
	@media (max-width: 480px) {
		.features-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
