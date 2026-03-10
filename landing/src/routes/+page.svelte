<script lang="ts">
	const features = [
		{
			icon: `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0f4c5c" stroke-width="2"><path d="M12 2L2 7l10 5 10-5-10-5z"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/></svg>`,
			title: 'AI Pipeline Generation',
			desc: 'Describe your analysis in natural language. Claude AI generates a complete Snakemake pipeline with Dockerfile, config, and metadata.'
		},
		{
			icon: `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0f4c5c" stroke-width="2"><rect x="2" y="3" width="20" height="18" rx="2"/><path d="M9 3v18"/><path d="M14 9l3 3-3 3"/></svg>`,
			title: 'Docker Execution',
			desc: 'Run pipelines in Docker containers on remote SSH servers. Fully reproducible environments with automated build and execution.'
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
			desc: 'Visualize BAM, VCF, BED, and GFF files with IGV.js. Supports reference genome selection for interactive genome browsing.'
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
	<title>Autopipe - Automate your bioinformatics pipelines</title>
	<meta name="description" content="AI-powered bioinformatics pipeline generation, execution, and sharing platform." />
</svelte:head>

<!-- Header -->
<header>
	<nav>
		<a href="/" class="logo">
			<img src="/logo.png" alt="Autopipe" />
			<span>Autopipe</span>
		</a>
		<div class="nav-links">
			<a href="#about">About</a>
			<a href="/plugins">Plugins</a>
		</div>
	</nav>
</header>

<!-- Hero -->
<section class="hero">
	<div class="hero-content">
		<div class="hero-text">
			<h1>Automate your bioinformatics pipelines effortlessly.</h1>
			<p>Generate, execute, and share reproducible Snakemake pipelines with AI. From raw data to publication-ready results in minutes.</p>
			<a href="/getting-started" class="btn-primary">Get Started</a>
		</div>
		<div class="hero-visual">
			<div class="hero-illustration">
				<div class="node n1"></div>
				<div class="node n2"></div>
				<div class="node n3"></div>
				<div class="node n4"></div>
				<div class="node n5"></div>
				<div class="connector c1"></div>
				<div class="connector c2"></div>
				<div class="connector c3"></div>
				<div class="connector c4"></div>
			</div>
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
	<p>Download the desktop app to create, execute, and manage bioinformatics pipelines with Claude AI.</p>
	<div class="download-buttons">
		<a href="/getting-started" class="btn-primary">Get Started</a>
		<a href="http://10.125.208.41:8090" target="_blank" rel="noopener" class="btn-secondary">Browse AutoPipeHub</a>
	</div>
</section>

<!-- Footer -->
<footer>
	<div class="footer-content">
		<a href="/" class="footer-logo">
			<img src="/logo.png" alt="Autopipe" />
			<span>Autopipe</span>
		</a>
		<span class="footer-copy">&copy; 2025 Autopipe</span>
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

	/* Hero illustration */
	.hero-visual {
		display: flex;
		justify-content: center;
		align-items: center;
	}
	.hero-illustration {
		position: relative;
		width: 400px;
		height: 300px;
	}
	.node {
		position: absolute;
		width: 60px;
		height: 60px;
		border-radius: 16px;
		background: rgba(255, 255, 255, 0.15);
		backdrop-filter: blur(10px);
		border: 2px solid rgba(255, 255, 255, 0.3);
		animation: float 3s ease-in-out infinite;
	}
	.n1 { top: 20px; left: 40px; animation-delay: 0s; background: rgba(16, 185, 129, 0.3); }
	.n2 { top: 60px; left: 170px; animation-delay: 0.5s; background: rgba(59, 130, 246, 0.3); }
	.n3 { top: 20px; right: 60px; animation-delay: 1s; background: rgba(245, 158, 11, 0.3); }
	.n4 { bottom: 60px; left: 100px; animation-delay: 1.5s; background: rgba(139, 92, 246, 0.3); }
	.n5 { bottom: 40px; right: 80px; animation-delay: 2s; background: rgba(236, 72, 153, 0.3); }
	.connector {
		position: absolute;
		height: 2px;
		background: rgba(255, 255, 255, 0.2);
		transform-origin: left center;
	}
	.c1 { top: 50px; left: 100px; width: 70px; transform: rotate(10deg); }
	.c2 { top: 90px; left: 230px; width: 80px; transform: rotate(-20deg); }
	.c3 { top: 160px; left: 160px; width: 100px; transform: rotate(30deg); }
	.c4 { bottom: 80px; left: 200px; width: 90px; transform: rotate(-10deg); }

	@keyframes float {
		0%, 100% { transform: translateY(0); }
		50% { transform: translateY(-10px); }
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
		.hero-content {
			grid-template-columns: 1fr;
			padding: 48px 24px;
		}
		.hero h1 {
			font-size: 2rem;
		}
		.hero-visual {
			display: none;
		}
		.features-grid {
			grid-template-columns: 1fr 1fr;
		}
		.download-buttons {
			flex-direction: column;
			align-items: center;
		}
	}
	@media (max-width: 480px) {
		.features-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
