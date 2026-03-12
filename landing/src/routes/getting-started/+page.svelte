<script lang="ts">
	import { env } from '$env/dynamic/public';

	const hubUrl = env.PUBLIC_HUB_URL;
</script>

<svelte:head>
	<title>Install Autopipe App</title>
</svelte:head>

<header>
	<nav>
		<a href="/" class="logo">
			<img src="/logo.png" alt="Autopipe" />
			<span>Autopipe</span>
		</a>
		<div class="nav-links">
			<a href={hubUrl} target="_blank" rel="noopener">Hub</a>
			<a href="/plugins">Plugins</a>
			<a href="/getting-started">Getting Started</a>
		</div>
	</nav>
</header>

<main>
	<div class="guide">
		<h1>Install Autopipe App</h1>
		<p class="intro">Set up Autopipe in a few minutes and start building bioinformatics pipelines with AI.</p>

		<!-- Step 1 -->
		<section class="step">
			<div class="step-number">1</div>
			<div class="step-content">
				<h2>Install Autopipe Desktop</h2>
				<p>Download the desktop app for your platform. It provides a GUI for configuration and runs as an MCP server for Claude Desktop.</p>
				<div class="options">
					<div class="option">
						<h4>macOS</h4>
						<p>Download the <code>.dmg</code> installer:</p>
						<div class="btn-group">
							<a href="https://download.autopipe.org/macOS/AutoPipe-macos-arm64.dmg" class="btn-sm">Download for Apple Silicon</a>
							<a href="https://download.autopipe.org/macOS/AutoPipe-macos-x64.dmg" class="btn-sm">Download for Intel</a>
						</div>
					</div>
					<div class="option">
						<h4>Windows</h4>
						<p>Download the <code>.exe</code> installer:</p>
						<a href="https://download.autopipe.org/windows/AutoPipe-Setup-v0.0.4.exe" class="btn-sm">Download for Windows</a>
					</div>
					<div class="option">
						<h4>Linux</h4>
						<p>Download the <code>.deb</code> package:</p>
						<a href="https://download.autopipe.org/linux/autopipe_0.0.4_amd64.deb" class="btn-sm">Download for Linux</a>
					</div>
				</div>
			</div>
		</section>

		<!-- Step 2 -->
		<section class="step">
			<div class="step-number">2</div>
			<div class="step-content">
				<h2>Configure Your Server</h2>
				<p>Open the Autopipe desktop app and set up your remote SSH server. This is where pipelines will be executed.</p>
				<div class="config-list">
					<div class="config-item">
						<span class="config-label">SSH Host</span>
						<span class="config-desc">Your compute server address</span>
					</div>
					<div class="config-item">
						<span class="config-label">SSH User</span>
						<span class="config-desc">Username for SSH connection</span>
					</div>
					<div class="config-item">
						<span class="config-label">Remote Repo Path</span>
						<span class="config-desc">Base path on the server — <code>pipelines/</code> and <code>pipelines_output/</code> will be created inside</span>
					</div>
				</div>
				<p class="hint">Make sure Docker is installed on your remote server. Pipelines run inside Docker containers for reproducibility.</p>
			</div>
		</section>

		<!-- Step 3 -->
		<section class="step">
			<div class="step-number">3</div>
			<div class="step-content">
				<h2>Connect GitHub</h2>
				<p>Go to the GitHub tab in the desktop app and link your GitHub account. This allows you to upload pipelines and publish them to AutoPipeHub.</p>
				<p class="hint">A GitHub Personal Access Token with <code>repo</code> scope is required.</p>
			</div>
		</section>

		<!-- Step 4 -->
		<section class="step">
			<div class="step-number">4</div>
			<div class="step-content">
				<h2>Save & Connect to Your MCP-Compatible AI App</h2>
				<p>Click <strong>"Save and Register & Minimize to Tray"</strong> to save your settings. Autopipe runs as an MCP (Model Context Protocol) server and works with any MCP-compatible AI app. We recommend <strong>Claude Desktop</strong> for the best experience.</p>
				<p class="hint">Restart your MCP-compatible AI app after launching Autopipe. Autopipe will continue running in the system tray.</p>
			</div>
		</section>

		<!-- Step 5 -->
		<section class="step">
			<div class="step-number">5</div>
			<div class="step-content">
				<h2>Create or Find a Pipeline</h2>
				<p>Open Claude Desktop and describe what you want to analyze. You can create a new pipeline from scratch:</p>
				<div class="example-chat">
					<div class="user-msg">Create a variant calling pipeline for paired-end WGS data using BWA-MEM2 and GATK HaplotypeCaller</div>
				</div>
				<p>Or find an existing pipeline — browse <a href={hubUrl} target="_blank" rel="noopener">AutoPipeHub</a> directly, or ask Claude to search for you:</p>
				<div class="example-chat">
					<div class="user-msg">Find a RNA-seq differential expression pipeline</div>
				</div>
				<p>When creating a new pipeline, Claude will use Autopipe to:</p>
				<ol>
					<li>Generate a <strong>Snakefile</strong> with the analysis workflow</li>
					<li>Create a <strong>Dockerfile</strong> with all required tools</li>
					<li>Write a <strong>config.yaml</strong> for your parameters</li>
					<li>Produce <strong>ro-crate-metadata.json</strong> for discoverability</li>
					<li>Add a <strong>README.md</strong> with usage instructions</li>
				</ol>
			</div>
		</section>

		<!-- Step 6 -->
		<section class="step">
			<div class="step-number">6</div>
			<div class="step-content">
				<h2>Run Your Pipeline</h2>
				<p>Once your pipeline is ready, ask Claude to execute it:</p>
				<div class="example-chat">
					<div class="user-msg">Build the Docker image and run a dry-run first, then execute on my samples in /data/wgs_samples</div>
				</div>
				<p class="hint">Claude will build the Docker image, run a dry-run to validate, and then execute the full pipeline on your server.</p>
			</div>
		</section>

		<!-- Step 7 -->
		<section class="step">
			<div class="step-number">7</div>
			<div class="step-content">
				<h2>View Results</h2>
				<p>After execution, view results directly in the browser viewer or download them locally.</p>
				<div class="example-chat">
					<div class="user-msg">Show me the results of my pipeline run</div>
				</div>
				<p class="hint">The viewer supports custom plugins for specialized visualizations. Browse available plugins on the <a href="/plugins">Plugins</a> page.</p>
			</div>
		</section>

		<!-- Step 8 -->
		<section class="step">
			<div class="step-number">8</div>
			<div class="step-content">
				<h2>Share on AutoPipeHub</h2>
				<p>Publish your pipeline to make it available for others:</p>
				<div class="example-chat">
					<div class="user-msg">Upload this pipeline to GitHub and publish it to AutoPipeHub</div>
				</div>
				<p>Your pipeline will be searchable on <a href={hubUrl} target="_blank" rel="noopener">AutoPipeHub</a> and downloadable by anyone.</p>
			</div>
		</section>

		<div class="next-steps">
			<h3>What's Next?</h3>
			<div class="next-grid">
				<a href={hubUrl} target="_blank" rel="noopener" class="next-card">
					<strong>AutoPipeHub</strong>
					<span>Browse and download community pipelines</span>
				</a>
				<a href="/plugins" class="next-card">
					<strong>Plugins</strong>
					<span>Extend the result viewer with custom plugins</span>
				</a>
				<a href="/plugins/guide" class="next-card">
					<strong>Plugin Development Guide</strong>
					<span>Learn how to create and publish your own plugins</span>
				</a>
			</div>
		</div>
	</div>
</main>

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
	:global(*) { margin: 0; padding: 0; box-sizing: border-box; }
	:global(body) {
		font-family: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
		color: #1a2332; background: #fff; line-height: 1.6;
	}

	header { position: sticky; top: 0; background: #fff; border-bottom: 1px solid #e5e7eb; z-index: 100; }
	nav { max-width: 1200px; margin: 0 auto; padding: 16px 24px; display: flex; align-items: center; justify-content: space-between; }
	.logo { display: flex; align-items: center; gap: 10px; text-decoration: none; color: #1a2332; font-weight: 700; font-size: 1.25rem; }
	.logo img { height: 32px; width: auto; }
	.nav-links { display: flex; gap: 32px; }
	.nav-links a { text-decoration: none; color: #4b5563; font-weight: 500; font-size: 0.95rem; }
	.nav-links a:hover { color: #1a2332; }

	main { max-width: 800px; margin: 0 auto; padding: 48px 24px 80px; }

	.guide h1 { font-size: 2.25rem; font-weight: 700; margin-bottom: 12px; }
	.intro { font-size: 1.1rem; color: #6b7280; margin-bottom: 48px; }

	.step { display: flex; gap: 24px; margin-bottom: 48px; }
	.step-number {
		flex-shrink: 0; width: 40px; height: 40px; border-radius: 50%;
		background: #0f4c5c; color: #fff; display: flex; align-items: center;
		justify-content: center; font-weight: 700; font-size: 1rem;
	}
	.step-content { flex: 1; }
	.step-content h2 { font-size: 1.25rem; font-weight: 600; margin-bottom: 8px; }
	.step-content p { color: #4b5563; margin-bottom: 12px; }
	.step-content ol { color: #4b5563; padding-left: 20px; margin-top: 8px; }
	.step-content ol li { margin-bottom: 4px; }

	.hint { font-size: 0.875rem; color: #9ca3af; }
	.hint code { background: #f3f4f6; padding: 2px 6px; border-radius: 4px; font-size: 0.8rem; }

	.code-block {
		background: #1a2332; color: #e5e7eb; padding: 14px 20px;
		border-radius: 8px; font-family: 'Fira Code', monospace; font-size: 0.9rem;
		margin: 12px 0;
	}

	.options { margin: 12px 0; display: flex; flex-direction: column; gap: 12px; }
	.option { background: #f9fafb; border: 1px solid #e5e7eb; border-radius: 8px; padding: 16px; }
	.option h4 { font-size: 0.95rem; margin-bottom: 4px; }
	.option p { font-size: 0.875rem; color: #6b7280; margin-bottom: 8px; }
	.btn-sm {
		display: inline-block; padding: 8px 16px; background: #0f4c5c; color: #fff;
		text-decoration: none; border-radius: 6px; font-size: 0.85rem; font-weight: 500;
	}
	.btn-sm:hover { background: #0d3d4a; }
	.btn-group { display: flex; gap: 8px; flex-wrap: wrap; }

	.config-list { margin: 12px 0; }
	.config-item {
		display: flex; justify-content: space-between; padding: 10px 0;
		border-bottom: 1px solid #f3f4f6;
	}
	.config-label { font-weight: 600; font-size: 0.9rem; }
	.config-desc { color: #9ca3af; font-size: 0.85rem; }

	.example-chat { margin: 12px 0; }
	.user-msg {
		background: #eff6ff; border: 1px solid #dbeafe; border-radius: 12px;
		padding: 12px 16px; font-size: 0.9rem; color: #1e40af;
	}

	.next-steps { margin-top: 64px; padding-top: 32px; border-top: 1px solid #e5e7eb; }
	.next-steps h3 { font-size: 1.25rem; font-weight: 600; margin-bottom: 20px; }
	.next-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; }
	.next-card {
		display: flex; flex-direction: column; gap: 4px; padding: 16px;
		border: 1px solid #e5e7eb; border-radius: 8px; text-decoration: none; color: #1a2332;
		transition: border-color 0.2s;
	}
	.next-card:hover { border-color: #0f4c5c; }
	.next-card strong { font-size: 0.95rem; }
	.next-card span { font-size: 0.8rem; color: #6b7280; }

	footer { border-top: 1px solid #e5e7eb; padding: 24px; }
	.footer-content { max-width: 1200px; margin: 0 auto; display: flex; align-items: center; justify-content: space-between; }
	.footer-logo { display: flex; align-items: center; gap: 8px; font-weight: 700; text-decoration: none; color: #1a2332; }
	.footer-logo img { height: 20px; }
	.footer-copy { color: #9ca3af; font-size: 0.8rem; }

	@media (max-width: 768px) {
		.step { flex-direction: column; gap: 12px; }
		.next-grid { grid-template-columns: 1fr; }
		.config-item { flex-direction: column; gap: 2px; }
	}
</style>
