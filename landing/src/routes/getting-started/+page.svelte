<script lang="ts">
	import { env } from '$env/dynamic/public';

	const hubUrl = env.PUBLIC_HUB_URL;
	let showModal = $state(false);
	let menuOpen = $state(false);
</script>

<svelte:head>
	<title>Getting Started - Autopipe</title>
</svelte:head>

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

<main>
	<div class="guide">
		<h1>Getting Started</h1>
		<p class="intro">Set up Autopipe in a few minutes and start building bioinformatics pipelines with AI.</p>

		<!-- Step 1 -->
		<section class="step">
			<div class="step-number">1</div>
			<div class="step-content">
				<h2>Install Autopipe Desktop</h2>
				<div class="prerequisites">
					<h4>Prerequisites</h4>
					<ul>
						<li>An MCP-compatible AI application on your local computer — we recommend <a href="https://claude.ai/download" target="_blank" rel="noopener" class="subtle-link">Claude Desktop</a></li>
						<li>Docker installed on the remote server where pipelines will run — <a href="https://docs.docker.com/engine/install/#installation-procedures-for-supported-platforms" target="_blank" rel="noopener" class="subtle-link">Install Docker</a>. Docker must be usable without <code>sudo</code>.</li>
					</ul>
					<details class="server-setup">
						<summary>Need to configure a new server?</summary>
						<div class="server-setup-content">
							<p>Run the following command on your server to automatically install SSH, Docker, Git, and verify the environment. The script will also display the SSH configuration values needed for the Autopipe desktop app.</p>
							<div class="code-block">curl -fsSL https://download.autopipe.org/setup.sh | bash</div>
						</div>
					</details>
				</div>
				<p>Download the desktop app for your platform. It provides a GUI for configuration and runs as an MCP server.</p>
				<div class="options">
					<div class="option">
						<h4>macOS</h4>
						<p>Download the <code>.dmg</code> installer:</p>
						<div class="btn-group">
							<a href="https://download.autopipe.org/macOS/AutoPipe-v0.0.11-macos-arm64.dmg" class="btn-sm">Download for Apple Silicon</a>
							<a href="https://download.autopipe.org/macOS/AutoPipe-v0.0.11-macos-x64.dmg" class="btn-sm">Download for Intel</a>
						</div>
					</div>
					<div class="option">
						<h4>Windows</h4>
						<p>Download the <code>.exe</code> installer:</p>
						<a href="https://download.autopipe.org/windows/AutoPipe-Setup-v0.0.11.exe" class="btn-sm">Download for Windows</a>
					</div>
					<div class="option">
						<h4>Linux</h4>
						<p>Download the <code>.deb</code> package:</p>
						<a href="https://download.autopipe.org/linux/autopipe_0.0.11_amd64.deb" class="btn-sm">Download for Linux</a>
					</div>
				</div>
				<div class="security-note">
					<p>Your operating system may block the app from running for security reasons. Run the appropriate command to allow it:</p>
					<div class="security-item">
						<span class="security-label">macOS</span> <span class="security-desc">— Run in Terminal:</span>
						<div class="code-block">xattr -cr /Applications/AutoPipe.app</div>
					</div>
					<div class="security-item">
						<span class="security-label">Windows</span> <span class="security-desc">— Run in PowerShell:</span>
						<div class="code-block">Unblock-File -Path "$HOME\Downloads\AutoPipe-Setup.exe"</div>
					</div>
				</div>
			</div>
		</section>

		<!-- Step 2 -->
		<section class="step">
			<div class="step-number">2</div>
			<div class="step-content">
				<h2>Get Autopipe Ready</h2>
				<p>Open the Autopipe desktop app and fill in your settings across the tabs.</p>
				<!-- svelte-ignore a11y_click_events_have_key_events -->
				<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
				<img src="/autopipe-guide.png" alt="Autopipe desktop app configuration" class="guide-img" onclick={() => showModal = true} />
				<ol>
					<li>In the <strong>SSH</strong> tab, enter your SSH host, username, and remote repository path for pipeline execution.
						<p style="margin-top:4px;font-size:13px">AutoPipe has been tested with Linux SSH servers. For Windows-based servers, WSL (Windows Subsystem for Linux) is required.</p>
					</li>
					<li>In the <strong>GitHub</strong> tab, connect your GitHub account. If you check <strong>"Create separate repository per pipeline"</strong>, each pipeline will be stored in its own repository. If unchecked, specify a shared repository name to store all pipelines in one repository.</li>
					<li>In the <strong>Setup</strong> tab, click <strong>"Save and Register & Minimize to Tray"</strong> to save your settings and register the MCP server.</li>
					<li>Restart your MCP-compatible AI app (e.g., Claude Desktop) to connect with AutoPipe.</li>
				</ol>
			</div>
		</section>

		{#if showModal}
			<!-- svelte-ignore a11y_click_events_have_key_events -->
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div class="modal-overlay" onclick={() => showModal = false}>
				<img src="/autopipe-guide.png" alt="Autopipe desktop app configuration" class="modal-img" />
			</div>
		{/if}

		<!-- Step 3 -->
		<section class="step">
			<div class="step-number">3</div>
			<div class="step-content">
				<h2>Create or Find a Pipeline</h2>
				<p>Open your AI app and describe what you want to analyze. You can create a new pipeline from scratch:</p>
				<div class="example-chat">
					<div class="user-msg">Use Autopipe to create a variant calling pipeline for paired-end WGS data using BWA-MEM2 and GATK HaplotypeCaller</div>
				</div>
				<p>Or find an existing pipeline — browse <a href={hubUrl} target="_blank" rel="noopener">AutoPipeHub</a> directly, or ask your AI to search for you:</p>
				<div class="example-chat">
					<div class="user-msg">Use Autopipe to find a RNA-seq differential expression pipeline</div>
				</div>
				<p>When creating a new pipeline, your AI app will use Autopipe to:</p>
				<ol>
					<li>Generate a <strong>Snakefile</strong> with the analysis workflow</li>
					<li>Create a <strong>Dockerfile</strong> with all required tools</li>
					<li>Write a <strong>config.yaml</strong> for your parameters</li>
					<li>Produce <strong>ro-crate-metadata.json</strong> for discoverability</li>
					<li>Add a <strong>README.md</strong> with usage instructions</li>
				</ol>
			</div>
		</section>

		<!-- Step 4 -->
		<section class="step">
			<div class="step-number">4</div>
			<div class="step-content">
				<h2>Run Your Pipeline</h2>
				<p>Once your pipeline is ready, ask your AI to execute it:</p>
				<div class="example-chat">
					<div class="user-msg">Build the Docker image and run a dry-run first, then execute on my samples in /data/wgs_samples</div>
				</div>
				<p class="hint">Autopipe will build the Docker image, run a dry-run to validate, and then execute the full pipeline on your server.</p>
			</div>
		</section>

		<!-- Step 5 -->
		<section class="step">
			<div class="step-number">5</div>
			<div class="step-content">
				<h2>View Results</h2>
				<p>After execution, view results directly in the browser viewer or download them locally.</p>
				<div class="example-chat">
					<div class="user-msg">Show me the results of my pipeline run with the viewer</div>
				</div>
				<p class="hint">The viewer supports custom plugins for specialized visualizations. Browse available plugins on the <a href="/plugins">Plugins</a> page.</p>
			</div>
		</section>

		<!-- Step 6 -->
		<section class="step">
			<div class="step-number">6</div>
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

	.hamburger { display: none; background: none; border: none; cursor: pointer; padding: 4px; flex-direction: column; gap: 5px; }
	.hamburger span { display: block; width: 24px; height: 2px; background: #1a2332; transition: transform 0.3s, opacity 0.3s; }
	.hamburger.open span:nth-child(1) { transform: translateY(7px) rotate(45deg); }
	.hamburger.open span:nth-child(2) { opacity: 0; }
	.hamburger.open span:nth-child(3) { transform: translateY(-7px) rotate(-45deg); }

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

	.prerequisites { background: #f8f9fa; border: 1px solid #e5e7eb; border-radius: 8px; padding: 16px 20px; margin-bottom: 16px; }
	.prerequisites h4 { font-size: 0.9rem; font-weight: 600; margin-bottom: 8px; color: #374151; }
	.prerequisites ul { margin: 0; padding-left: 20px; }
	.prerequisites li { font-size: 0.9rem; color: #4b5563; line-height: 1.8; }
	.server-setup { margin-top: 12px; border-top: 1px solid #e5e7eb; padding-top: 12px; }
	.server-setup summary { font-size: 0.85rem; font-weight: 500; color: #0f4c5c; cursor: pointer; user-select: none; }
	.server-setup summary:hover { color: #0d3d4a; }
	.server-setup-content { margin-top: 10px; }
	.server-setup-content p { font-size: 0.85rem; color: #6b7280; margin-bottom: 8px; }
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

	.guide-img {
		width: 100%; border-radius: 8px; border: 1px solid #e5e7eb;
		margin: 12px 0; cursor: pointer; transition: opacity 0.2s;
	}
	.guide-img:hover { opacity: 0.85; }
	.modal-overlay {
		position: fixed; top: 0; left: 0; width: 100%; height: 100%;
		background: rgba(0, 0, 0, 0.8); display: flex; align-items: center;
		justify-content: center; z-index: 1000; cursor: pointer;
	}
	.modal-img { max-width: 90%; max-height: 90%; border-radius: 8px; }
	.security-note {
		background: #fefce8; border: 1px solid #fef08a; border-radius: 8px;
		padding: 16px; margin-top: 16px;
	}
	.security-note > p { font-size: 0.875rem; color: #6b7280; margin-bottom: 12px; }
	.security-item { margin-bottom: 8px; }
	.security-label { font-weight: 600; font-size: 0.9rem; }
	.security-desc { font-size: 0.85rem; color: #6b7280; }
	.subtle-link { color: inherit; text-decoration: underline; }
	.subtle-link:hover { opacity: 0.7; }

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
		.hamburger { display: flex; }
		.nav-links { display: none; position: absolute; top: 100%; left: 0; right: 0; background: #fff; flex-direction: column; padding: 16px 24px; gap: 16px; border-bottom: 1px solid #e5e7eb; box-shadow: 0 4px 12px rgba(0,0,0,0.08); }
		.nav-links.open { display: flex; }
		nav { position: relative; }
		.step { flex-direction: column; gap: 12px; }
		.next-grid { grid-template-columns: 1fr; }
	}
</style>
