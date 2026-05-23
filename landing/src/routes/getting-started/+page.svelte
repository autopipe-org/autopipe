<script lang="ts">
	import { env } from '$env/dynamic/public';
	import { onMount } from 'svelte';

	const hubUrl = env.PUBLIC_HUB_URL;
	const downloadUrl = env.PUBLIC_DOWNLOAD_URL ?? 'https://download.autopipe.org';
	const version = env.PUBLIC_AUTOPIPE_VERSION ?? 'v0.0.14';
	const versionBare = version.replace(/^v/, '');

	let showModal = $state(false);
	let menuOpen = $state(false);

	// Section 2 (server) and Section 3 (AI app machine) keep independent
	// OS selections so the same page works for any combination of
	// AI-app machine × server machine.
	type ServerOs = 'mac' | 'windows' | 'linux';
	type ClientOs = 'mac' | 'windows' | 'linux';
	let serverOs = $state<ServerOs>('mac');
	let clientOs = $state<ClientOs>('mac');

	// Open the troubleshooting "What is the tray?" disclosure when the user
	// clicks the inline "tray" link in Section 4. Scrolls to the section 7
	// header (not the details element itself) so the user lands at the
	// section heading and the disclosure opens just below.
	function openTrayInfo(e: MouseEvent) {
		const details = document.getElementById('what-is-tray') as HTMLDetailsElement | null;
		const section = document.getElementById('troubleshooting');
		if (!section || !details) return;

		e.preventDefault();
		details.open = true;

		// Account for the sticky page header so Section 7's heading isn't
		// hidden underneath it.
		const headerOffset = 80;
		const top = section.getBoundingClientRect().top + window.scrollY - headerOffset;
		window.scrollTo({ top, behavior: 'smooth' });
		history.replaceState(null, '', '#troubleshooting');
	}

	// Add a "Copy" button (far right) to every command block (.code-block).
	// A MutationObserver covers blocks added later when the OS tabs switch.
	onMount(() => {
		function addCopy(block: HTMLElement) {
			if (block.querySelector('.code-copy')) return;
			const cmd = block.textContent?.trim() ?? '';
			const btn = document.createElement('button');
			btn.type = 'button';
			btn.className = 'code-copy';
			btn.textContent = 'Copy';
			btn.addEventListener('click', async () => {
				try {
					await navigator.clipboard.writeText(cmd);
					btn.textContent = 'Copied!';
					setTimeout(() => { btn.textContent = 'Copy'; }, 1500);
				} catch {}
			});
			block.appendChild(btn);
		}
		const scan = (root: ParentNode) =>
			root.querySelectorAll('.code-block').forEach((b) => addCopy(b as HTMLElement));
		scan(document);
		const obs = new MutationObserver((muts) => {
			for (const m of muts)
				for (const n of m.addedNodes) {
					if (!(n instanceof HTMLElement)) continue;
					if (n.classList.contains('code-block')) addCopy(n);
					else scan(n);
				}
		});
		obs.observe(document.body, { childList: true, subtree: true });
		return () => obs.disconnect();
	});
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
		<p class="intro">Set up Autopipe in two parts: a <strong>server machine</strong> that runs your pipelines, and an <strong>AI-app machine</strong> that talks to it.</p>

		<!-- ============================================================ -->
		<!-- Section 1 - How AutoPipe is structured                       -->
		<!-- ============================================================ -->
		<section class="step">
			<div class="step-number">1</div>
			<div class="step-content">
				<h2>How AutoPipe is structured</h2>
				<p>Two machines are involved in every Autopipe setup:</p>

				<div class="diagram">
					<div class="diagram-box">
						<strong>Server machine</strong>
						<span>Docker + ssh + pipeline files</span>
					</div>
					<div class="diagram-arrow">
						<span class="arrow-label">SSH</span>
						<span class="arrow-line">⟷</span>
					</div>
					<div class="diagram-box">
						<strong>AI app machine</strong>
						<span>Claude Desktop + Autopipe app</span>
					</div>
				</div>

				<p>They can be the <em>same</em> physical computer (e.g. set up your Mac to play both roles) or two <em>different</em> ones (e.g. a Mac running Claude Desktop, a separate Linux box doing the work).</p>
				<p>The next two sections set each machine up independently. <strong>Pick the OS tab that matches your machine in each section</strong> - they don't have to be the same.</p>
			</div>
		</section>

		<!-- ============================================================ -->
		<!-- Section 2 - Set up the SERVER machine                        -->
		<!-- ============================================================ -->
		<section class="step">
			<div class="step-number">2</div>
			<div class="step-content">
				<h2>Set up your server machine</h2>
				<p>This is the machine that will actually run pipelines (build Docker images, execute Snakemake, store results). It needs Docker and an SSH server.</p>

				<div class="tabs" role="tablist">
					<button class="tab" class:active={serverOs === 'mac'} onclick={() => serverOs = 'mac'} role="tab" aria-selected={serverOs === 'mac'}>macOS</button>
					<button class="tab" class:active={serverOs === 'windows'} onclick={() => serverOs = 'windows'} role="tab" aria-selected={serverOs === 'windows'}>Windows (WSL)</button>
					<button class="tab" class:active={serverOs === 'linux'} onclick={() => serverOs = 'linux'} role="tab" aria-selected={serverOs === 'linux'}>Linux</button>
				</div>

				<div class="tab-panel">
					{#if serverOs === 'mac'}
						<h3>2.1 Install Docker Desktop</h3>
						<p>Install Docker Desktop for your Mac chip. Pick <strong>Apple Silicon</strong> if your Mac uses an M-series chip, or <strong>Intel</strong> otherwise.</p>
						<p><a href="https://www.docker.com/products/docker-desktop/" target="_blank" rel="noopener" class="btn-sm">Open Docker Desktop downloads</a></p>
						<p>After installation, launch Docker Desktop. When the <strong>whale icon</strong> appears in the macOS menu bar (top right of the screen), Docker is ready to use. Leave it running - quitting it will stop your pipelines from running. Click the icon to confirm Docker is active:</p>
						<img src="/mac-docker-running.png" alt="Docker Desktop whale icon in the macOS menu bar showing Running in Resource Saver mode" class="screenshot-sm" />
						<p class="hint">If the whale icon shows "Docker Desktop is starting…", wait until it finishes - pipelines will fail until Docker is fully running.</p>

						<h3>2.2 Enable Remote Login</h3>
						<ol>
							<li>Open <strong>System Settings</strong></li>
							<li>Go to <strong>General → Sharing → Advanced</strong></li>
							<li>Toggle <strong>Remote Login</strong> on (enter your Mac password if asked)</li>
						</ol>
						<p class="hint">On older macOS the toggle is directly under <strong>General → Sharing</strong> without the Advanced submenu.</p>

						<h3>2.3 Run the setup script</h3>
						<p>Open Terminal and run:</p>
						<div class="code-block">curl -fsSL https://download.autopipe.org/setup.sh | bash</div>
						<p>The script installs everything Autopipe needs and prints the SSH info (Host, Port, User, Repo Path) at the end. Note these values for Section 4 - once you have them, you can close the Terminal.</p>
					{:else if serverOs === 'windows'}
						<h3 class="section-label">Setup video</h3>
						<p class="hint">Watch the whole setup in the video above. For the exact commands to type and the order to run them in, expand <strong>Detailed steps</strong> below.</p>
						<!-- svelte-ignore a11y_media_has_caption -->
						<video src="/autopipe_win_server.mp4" class="setup-video" controls preload="metadata" playsinline></video>

						<details class="detailed-steps">
							<summary class="section-label">Detailed steps</summary>
						<h3>2.1 Install WSL with Ubuntu</h3>
						<p>Open <strong>PowerShell as Administrator</strong> (Start menu → search "PowerShell" → right-click → <em>Run as administrator</em>) and run:</p>
						<div class="code-block">wsl --install</div>

						<p>If you see a message that <strong>Ubuntu is being installed</strong>, the installation has succeeded. The same PowerShell window will switch to an Ubuntu setup prompt once it finishes. Continue to step 2.2.</p>

						<p>If instead Windows tells you to <strong>restart your computer</strong>, restart the computer, then open <strong>PowerShell as Administrator</strong> again and run <code>wsl --install</code> once more. This time you should see the "Ubuntu is being installed" message. Continue to step 2.2.</p>

						<p class="hint">Microsoft's official guide: <a href="https://learn.microsoft.com/windows/wsl/install" target="_blank" rel="noopener" class="subtle-link">Install WSL on Windows</a>. If <code>wsl --install</code> still fails after a restart, see the <a href="#troubleshooting" class="subtle-link">troubleshooting section</a>.</p>

						<h3>2.2 Create your Ubuntu user</h3>
						<p>The same window now shows Ubuntu's first-run prompt. Fill in:</p>
						<ul>
							<li><strong>Username</strong>: lowercase letters only. Do <strong>not</strong> use <code>root</code>.</li>
							<li><strong>Password</strong>: nothing appears as you type - that's normal. Type it twice.</li>
							<li>You may also see "<em>Would you like to opt-in to platform metrics?</em>" - answer with whichever you prefer (<code>y</code> or <code>n</code>); it does not affect Autopipe.</li>
						</ul>

						<h3>2.3 Run the setup script (first pass)</h3>
						<p>Still in the same Ubuntu prompt, run:</p>
						<div class="code-block">curl -fsSL https://download.autopipe.org/setup.sh | bash</div>
						<p>The script installs everything Autopipe needs. At the end it adds your user to the <code>docker</code> group and prints a <strong>"log out and back in for docker group changes to take effect"</strong> message. This is expected - finish with the next step.</p>

						<h3>2.4 Re-enter WSL and run the script again</h3>
						<ol>
							<li>Close the PowerShell window completely (the WSL session ends with it).</li>
							<li>Open <strong>PowerShell</strong> again (regular permissions are fine this time) and type:
								<div class="code-block">wsl</div>
							</li>
							<li>You're back in Ubuntu. Paste the same <code>curl</code> command and run it once more:
								<div class="code-block">curl -fsSL https://download.autopipe.org/setup.sh | bash</div>
							</li>
						</ol>
						<p>This second run finishes the setup and prints the SSH info (Host, Port, User, Repo Path). <strong>Note these values</strong> - you'll paste them into Autopipe in Section 4.</p>

						<h3>2.5 Keep this PowerShell window open</h3>
						<p>WSL only stays alive while a session is open. If you close this window, WSL shuts down and Autopipe can't connect over SSH. <strong>Minimize is fine; don't close.</strong></p>
						<p class="hint">If you do close it later by accident, just open PowerShell, type <code>wsl</code>, and leave the window minimized - Autopipe's setup script already configured the SSH server to start automatically when WSL launches.</p>
						</details>
					{:else if serverOs === 'linux'}
						<h3>2.1 SSH into your Linux server</h3>
						<div class="code-block">ssh user@server-address</div>

						<h3>2.2 Run the setup script</h3>
						<div class="code-block">curl -fsSL https://download.autopipe.org/setup.sh | bash</div>
						<p>The script installs everything Autopipe needs and prints the SSH info (Host, Port, User, Repo Path) at the end. Note these values for Section 4.</p>
						<p class="hint">Need to install Docker manually first? See the <a href="https://docs.docker.com/engine/install/#installation-procedures-for-supported-platforms" target="_blank" rel="noopener" class="subtle-link">official Docker installation guide</a>.</p>
					{/if}
				</div>
			</div>
		</section>

		<!-- ============================================================ -->
		<!-- Section 3 - Set up the AI APP machine                        -->
		<!-- ============================================================ -->
		<section class="step">
			<div class="step-number">3</div>
			<div class="step-content">
				<h2>Set up your AI-app machine</h2>
				<p>This is the laptop or desktop where you'll chat with Claude Desktop. If it's the same machine as your server, you're just installing two more apps on it.</p>

				<div class="tabs" role="tablist">
					<button class="tab" class:active={clientOs === 'mac'} onclick={() => clientOs = 'mac'} role="tab" aria-selected={clientOs === 'mac'}>macOS</button>
					<button class="tab" class:active={clientOs === 'windows'} onclick={() => clientOs = 'windows'} role="tab" aria-selected={clientOs === 'windows'}>Windows</button>
					<button class="tab" class:active={clientOs === 'linux'} onclick={() => clientOs = 'linux'} role="tab" aria-selected={clientOs === 'linux'}>Linux</button>
				</div>

				<div class="tab-panel">
					{#if clientOs === 'linux'}
						<h3>3.1 Pick an MCP-compatible AI client</h3>
						<p><strong>Claude Desktop is not available on Linux.</strong> Use any MCP-compatible AI application that supports Linux as your client for Autopipe.</p>
					{:else}
						<h3>3.1 Install Claude Desktop</h3>
						<p>Autopipe works with any MCP-compatible AI app. We recommend Claude Desktop:</p>
						<p><a href="https://claude.ai/download" target="_blank" rel="noopener" class="btn-sm">Open Claude Desktop downloads</a></p>
						<p class="hint">If you use Codex CLI, Gemini, or another MCP client, the rest of this guide still applies.</p>
					{/if}

					<h3>3.2 Install the Autopipe desktop app</h3>
					{#if clientOs === 'mac'}
						<p>Download the macOS installer for your chip:</p>
						<div class="btn-group">
							<a href="{downloadUrl}/macOS/AutoPipe-{version}-macos-arm64.dmg" class="btn-sm">Download for Apple Silicon</a>
							<a href="{downloadUrl}/macOS/AutoPipe-{version}-macos-x64.dmg" class="btn-sm">Download for Intel</a>
						</div>
						<p class="hint">Drag <strong>AutoPipe.app</strong> into the Applications folder.</p>

						<div class="security-note">
							<p>macOS may block the app the first time you open it ("unidentified developer"). Allow it once with this Terminal command:</p>
							<div class="code-block">xattr -cr /Applications/AutoPipe.app</div>
						</div>
					{:else if clientOs === 'windows'}
						<p>Download the Windows installer:</p>
						<a href="{downloadUrl}/windows/AutoPipe-Setup-{version}.exe" class="btn-sm">Download for Windows</a>
						<p class="hint">Run the installer. If SmartScreen blocks it, click <strong>More info → Run anyway</strong>.</p>

						<div class="security-note">
							<p>If Windows still blocks the file, unblock it once in PowerShell:</p>
							<div class="code-block">{`Unblock-File -Path "$HOME\\Downloads\\AutoPipe-Setup.exe"`}</div>
						</div>
					{:else if clientOs === 'linux'}
						<p>Download the Linux <code>.deb</code> package (Debian / Ubuntu):</p>
						<a href="{downloadUrl}/linux/autopipe_{versionBare}_amd64.deb" class="btn-sm">Download for Linux (.deb)</a>
						<p class="hint">Install with <code>sudo apt install ./autopipe_{versionBare}_amd64.deb</code>, then launch from your application menu or run <code>autopipe</code> from a terminal.</p>
					{/if}
				</div>
			</div>
		</section>

		<!-- ============================================================ -->
		<!-- Section 4 - Configure AutoPipe (with annotated screenshot)   -->
		<!-- ============================================================ -->
		<section class="step">
			<div class="step-number">4</div>
			<div class="step-content">
				<h2>Configure Autopipe</h2>
				<p>Open the Autopipe app on your AI-app machine. Walk through the setup screen top to bottom.</p>

				<!-- svelte-ignore a11y_click_events_have_key_events -->
				<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
				<img src="/autopipe-guide.png" alt="Autopipe configuration screen with annotations" class="guide-img" onclick={() => showModal = true} />

				<h3>4.1 SSH connection</h3>
				<p>Paste the values that the setup script printed in Section 2.</p>

				<h3>4.2 GitHub &amp; Save</h3>
				<p>The five numbered callouts on the screenshot above map to the steps below:</p>
				<ol class="callouts">
					<li><span class="num">①</span> Click <strong>Connect GitHub</strong> - a code appears, and the GitHub login window opens.</li>
					<li><span class="num">②</span> Log in to GitHub and enter the code in that window. Your username appears here once you're authenticated.</li>
					<li><span class="num">③</span> Enter the repository name where your pipelines will be uploaded.</li>
					<li><span class="num">④</span> Click <strong>Save and Register</strong> to register Autopipe with your AI app, then launch the AI app.</li>
					<li><span class="num">⑤</span> Don't quit Autopipe - click <strong>Move to <a href="#what-is-tray" class="inline-link" onclick={openTrayInfo}>tray</a></strong>.</li>
				</ol>
			</div>
		</section>

		{#if showModal}
			<!-- svelte-ignore a11y_click_events_have_key_events -->
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div class="modal-overlay" onclick={() => showModal = false}>
				<img src="/autopipe-guide.png" alt="Autopipe configuration screen" class="modal-img" />
			</div>
		{/if}

		<!-- ============================================================ -->
		<!-- Section 5 - Restart your AI app                              -->
		<!-- ============================================================ -->
		<section class="step">
			<div class="step-number">5</div>
			<div class="step-content">
				<h2>Restart your AI app</h2>
				<p>If Claude Desktop (or any other AI app you're using) was already running before you finished Section 4, you <strong>must fully quit it</strong> - closing the window is not enough. The MCP server list is only re-read when the app starts fresh, so a still-running instance will not see Autopipe.</p>
				<ul>
					{#if clientOs === 'mac'}
						<li><strong>macOS</strong>: click the Claude icon in the <strong>top menu bar</strong> → <strong>Quit Claude</strong>. The icon should disappear. Then reopen via Spotlight.</li>
					{:else if clientOs === 'windows'}
						<li><strong>Windows</strong>: open the system <strong>tray</strong> (the area at the bottom-right of the taskbar - click the <strong>^</strong> arrow if the icon is hidden), right-click the Claude icon → <strong>Quit</strong>. The icon must disappear from the tray, not just from the taskbar. Then reopen from the Start menu.</li>
					{:else if clientOs === 'linux'}
						<li><strong>Linux</strong>: fully terminate your MCP client (<code>Ctrl+C</code> in a terminal-based client like Codex CLI, or quit the app from your desktop environment), then start it again. A reload alone may not pick up the new MCP server entry.</li>
					{/if}
				</ul>
				<p class="hint">If you're not sure whether Claude was running, quit and relaunch anyway - it's safe and only takes a few seconds.</p>
			</div>
		</section>

		<!-- ============================================================ -->
		<!-- Section 6 - Run your first pipeline                          -->
		<!-- ============================================================ -->
		<section class="step">
			<div class="step-number">6</div>
			<div class="step-content">
				<h2>Run your first pipeline</h2>
				<p>You're done. Talk to your AI - for example, you can find a pipeline already published in the Autopipe registry:</p>
				<div class="example-chat">
					<div class="user-msg">Use Autopipe to find a single cell downstream analysis pipeline.</div>
				</div>
				<p>Or build a brand-new pipeline from a description:</p>
				<div class="example-chat">
					<div class="user-msg">Use Autopipe to create a variant calling pipeline for paired-end WGS data using BWA-MEM2 and GATK HaplotypeCaller.</div>
				</div>
				<p>Autopipe will:</p>
				<ol>
					<li>Search <a href={hubUrl} target="_blank" rel="noopener">AutoPipeHub</a> or generate a Snakemake pipeline.</li>
					<li>Build the Docker image on the server.</li>
					<li>Run a dry-run, then execute the full pipeline.</li>
					<li>Show results in the in-browser viewer.</li>
				</ol>
				<p>When the run finishes, ask <em>"Show me the results"</em> - Autopipe summarizes the outputs and points you to the visual viewer.</p>
			</div>
		</section>

		<!-- ============================================================ -->
		<!-- Section 7 - Troubleshooting                                  -->
		<!-- ============================================================ -->
		<section class="step" id="troubleshooting">
			<div class="step-number">7</div>
			<div class="step-content">
				<h2>Troubleshooting</h2>

				<!-- ── Common (any OS) ────────────────────────────────────── -->
				<details class="ts-item" id="what-is-tray">
					<summary>What is the "tray", and where is it?</summary>
					<p>The <strong>tray</strong> (also called the system tray or menu bar) is the strip of small icons your OS uses to show background apps that are still running but don't have a visible window.</p>

					<p><strong>macOS</strong> - the tray is the right-hand side of the menu bar at the <strong>top of the screen</strong>. After clicking <em>Move to tray</em>, look for the small Autopipe "A" icon there:</p>
					<img src="/mac-tray.png" alt="macOS menu bar with the Autopipe icon visible in the tray area" class="screenshot-sm" />

					<p><strong>Windows</strong> - the tray is the area on the right of the taskbar at the <strong>bottom of the screen</strong>. Some icons are hidden behind the small <strong>^</strong> arrow; click it to expand. The Autopipe "A" icon appears there once you click <em>Move to tray</em>:</p>
					<img src="/win-tray.png" alt="Windows system tray with the Autopipe icon visible after expanding the hidden icons panel" class="screenshot-square" />

					<p class="hint">If you can't find the Autopipe icon in the tray, the app may have been quit instead of minimised. Reopen it from Applications (macOS) or the Start menu (Windows) and click <em>Move to tray</em> again.</p>
				</details>

				<details class="ts-item">
					<summary>"SSH connection failed" or "SSH handshake failed"</summary>
					<ul>
						<li>Double-check the password (typos are common).</li>
						<li><strong>macOS server</strong>: System Settings → General → Sharing → Advanced → Remote Login is on?</li>
						<li><strong>Windows server (WSL)</strong>: Is the PowerShell / WSL window still open? Closing it stops the SSH server.</li>
						<li>Test the SSH connection directly from a terminal: <code>ssh &lt;user&gt;@&lt;host&gt;</code>.</li>
					</ul>
				</details>

				<details class="ts-item">
					<summary>"private or unavailable" - pipeline download fails</summary>
					<p>The remote server is missing the GitHub CLI (<code>gh</code>) or has a PATH issue. Re-run the setup script on the server:</p>
					<div class="code-block">curl -fsSL https://download.autopipe.org/setup.sh | bash</div>
				</details>

				<details class="ts-item">
					<summary>"Docker is not in PATH"</summary>
					<p>Re-run the setup script. If Docker is still missing on macOS or Linux, add it to a non-interactive PATH:</p>
					<div class="code-block">echo 'export PATH="$HOME/bin:/usr/local/bin:$PATH"' &gt;&gt; ~/.zshenv</div>
				</details>

				<details class="ts-item">
					<summary>Autopipe doesn't show up in your AI app</summary>
					<ul>
						<li>Quit your AI app completely (not just close the window) and reopen it.</li>
						<li>In Autopipe, click <strong>Save and Register</strong> again.</li>
						<li>Confirm Autopipe is still running in the menu bar (macOS) or system tray (Windows).</li>
						<li><strong>Claude Desktop</strong>: open the <strong>+</strong> menu → <strong>Connectors</strong> and toggle <strong>autopipe</strong> off and then back on. This re-registers the tools and usually makes them reappear without restarting the app.</li>
					</ul>
					<img src="/image.png" alt="Claude Desktop Connectors menu with the autopipe connector toggled on" class="screenshot-md" />
				</details>

				<!-- ── macOS-specific ─────────────────────────────────────── -->
				<details class="ts-item">
					<summary>macOS: Docker Desktop suddenly stopped</summary>
					<ul>
						<li>Check the menu bar - if there's no whale icon, Docker quit. Reopen via Spotlight.</li>
						<li>Crashes often? Docker Desktop → Settings → Resources → bump the memory limit.</li>
					</ul>
				</details>

				<!-- ── Windows-specific ───────────────────────────────────── -->
				<details class="ts-item">
					<summary>Windows: SSH worked once, then handshake fails after restarting WSL</summary>
					<p>WSL 2 assigns the Linux VM a <strong>new IP address every time it boots</strong> (typically in the <code>172.x.x.x</code> range). The IP that <code>setup.sh</code> printed the first time is no longer valid after a WSL restart, so the address you saved into Autopipe's SSH tab is now wrong.</p>
					<p><strong>Fix (one-time, recommended):</strong> change the <strong>Host</strong> field in Autopipe's SSH tab to <code>127.0.0.1</code> (or <code>localhost</code>). WSL 2 forwards <code>localhost</code> traffic from Windows into the Linux VM automatically, so the address never changes again.</p>
					<p><strong>Alternative:</strong> grab the new IP and update the Host field. Inside WSL:</p>
					<div class="code-block">hostname -I</div>
					<p>The first token in the output is your current WSL IP. Paste it into Autopipe's Host field and click Save and Register.</p>
					<p>If <code>service ssh status</code> reports <code>inactive</code>, start it manually with <code>sudo service ssh start</code>, or re-run the setup script - it configures WSL to start the SSH server automatically on every boot.</p>
				</details>

				<details class="ts-item">
					<summary>Windows: Ubuntu window keeps closing</summary>
					<p>Reinstall WSL:</p>
					<div class="code-block">{`wsl --unregister Ubuntu
wsl --install -d Ubuntu`}</div>
				</details>

				<details class="ts-item">
					<summary>Windows: <code>wsl --install</code> fails or "Virtual Machine Platform" / "virtualization" error</summary>
					<p>Hardware virtualization needs to be enabled in your PC's BIOS for WSL 2 to run. On most modern PCs it's already enabled - if it isn't, the WSL installer surfaces the error.</p>
					<p>Verify the current state in PowerShell:</p>
					<div class="code-block">systeminfo | findstr /i "Virtualization"</div>
					<p>If the line shows <code>Virtualization Enabled in Firmware: No</code>:</p>
					<ol>
						<li>Reboot and enter BIOS (usually F2 / F10 / Del during the boot logo).</li>
						<li>Find the CPU settings and enable <strong>Intel VT-x</strong> (Intel) or <strong>AMD-V</strong> / <strong>SVM</strong> (AMD).</li>
						<li>Save, exit BIOS, and run <code>wsl --install</code> again from an Administrator PowerShell.</li>
					</ol>
				</details>
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

	main { max-width: 1100px; margin: 0 auto; padding: 48px 32px 80px; }

	.guide h1 { font-size: 2.25rem; font-weight: 700; margin-bottom: 12px; }
	.intro { font-size: 1.1rem; color: #6b7280; margin-bottom: 48px; }

	.step { display: flex; gap: 24px; margin-bottom: 48px; }
	.step-number {
		flex-shrink: 0; width: 40px; height: 40px; border-radius: 50%;
		background: #0f4c5c; color: #fff; display: flex; align-items: center;
		justify-content: center; font-weight: 700; font-size: 1rem;
	}
	.step-content { flex: 1; min-width: 0; }
	.step-content h2 { font-size: 1.35rem; font-weight: 600; margin-bottom: 12px; }
	.step-content h3 { font-size: 1rem; font-weight: 600; margin: 20px 0 8px; color: #1a2332; }
	.step-content p { color: #4b5563; margin-bottom: 12px; }
	.step-content ol, .step-content ul { color: #4b5563; padding-left: 20px; margin: 8px 0 12px; }
	.step-content ol li, .step-content ul li { margin-bottom: 4px; }

	/* Section 1 diagram */
	.diagram {
		display: flex; gap: 12px; align-items: stretch; justify-content: center;
		background: #f8f9fa; border: 1px solid #e5e7eb; border-radius: 12px;
		padding: 20px; margin: 16px 0;
	}
	.diagram-box {
		flex: 1; display: flex; flex-direction: column; align-items: center;
		text-align: center; padding: 16px; background: #fff;
		border: 1px solid #e5e7eb; border-radius: 8px;
	}
	.diagram-box strong { font-size: 0.95rem; color: #1a2332; }
	.diagram-box span { font-size: 0.8rem; color: #6b7280; margin-top: 4px; }
	.diagram-arrow {
		display: flex; flex-direction: column; align-items: center;
		justify-content: center; min-width: 60px; color: #6b7280;
	}
	.arrow-label { font-size: 0.75rem; font-weight: 500; }
	.arrow-line { font-size: 1.5rem; line-height: 1; }

	/* OS tabs (Section 2 + 3) */
	.tabs {
		display: flex; gap: 4px; margin: 16px 0 0;
		border-bottom: 1px solid #e5e7eb;
	}
	.tab {
		padding: 8px 16px; background: none; border: none; cursor: pointer;
		font-size: 0.9rem; color: #6b7280; font-weight: 500;
		border-bottom: 2px solid transparent; transition: color 0.2s, border-color 0.2s;
		font-family: inherit;
	}
	.tab:hover { color: #1a2332; }
	.tab.active {
		color: #0f4c5c; border-bottom-color: #0f4c5c; font-weight: 600;
	}
	.tab-panel { padding-top: 12px; }

	/* Annotated callouts */
	.callouts { list-style: none; padding: 0; margin: 12px 0; }
	.callouts li {
		display: flex; gap: 10px; align-items: flex-start;
		padding: 10px 12px; background: #f8f9fa; border-radius: 8px;
		margin-bottom: 6px; color: #374151; font-size: 0.92rem;
	}
	.callouts .num {
		display: inline-flex; align-items: center; justify-content: center;
		font-weight: 700; color: #0f4c5c; min-width: 24px;
	}

	/* Troubleshooting items */
	.ts-item {
		background: #f8f9fa; border: 1px solid #e5e7eb; border-radius: 8px;
		padding: 10px 14px; margin-bottom: 8px;
	}
	.ts-item summary {
		cursor: pointer; font-weight: 500; color: #1a2332;
		font-size: 0.95rem; user-select: none;
	}
	.ts-item summary:hover { color: #0f4c5c; }
	.ts-item[open] summary { margin-bottom: 8px; }
	.ts-item p, .ts-item ul { font-size: 0.9rem; color: #4b5563; }

	.hint { font-size: 0.875rem; color: #9ca3af; }
	.hint code { background: #f3f4f6; padding: 2px 6px; border-radius: 4px; font-size: 0.8rem; }

	.code-block {
		position: relative;
		background: #1a2332; color: #e5e7eb; padding: 14px 88px 14px 20px;
		border-radius: 8px; font-family: 'Fira Code', monospace; font-size: 0.9rem;
		margin: 12px 0; white-space: pre; overflow-x: auto;
	}
	/* The Copy button is created in JS (not in markup), so it must be :global —
	   otherwise Svelte scopes the selector and it never matches the button. */
	:global(.code-copy) {
		position: absolute; top: 8px; right: 8px;
		display: inline-flex; align-items: center; justify-content: center;
		min-width: 60px; padding: 6px 14px;
		background: #334155; color: #e5e7eb; border: 1px solid #475569;
		border-radius: 6px; font-size: 0.8rem; font-weight: 500; cursor: pointer;
		font-family: -apple-system, BlinkMacSystemFont, sans-serif;
	}
	:global(.code-copy:hover) { background: #475569; }

	/* Windows setup walkthrough video (shown above the written steps). */
	.setup-video {
		width: 100%; max-width: 760px; display: block;
		border-radius: 10px; border: 1px solid #e5e7eb; margin: 8px 0 20px;
	}

	/* Distinct, larger labels separating the "Setup video" and "Detailed
	   steps" blocks from the smaller numbered sub-steps (2.1, 2.2 …). */
	.step-content .section-label {
		font-size: 1.2rem; font-weight: 700; color: #0f4c5c; margin: 24px 0 12px;
	}
	.detailed-steps { margin-bottom: 12px; }
	.detailed-steps > summary { cursor: pointer; }

	.btn-sm {
		display: inline-block; padding: 8px 16px; background: #0f4c5c; color: #fff;
		text-decoration: none; border-radius: 6px; font-size: 0.85rem; font-weight: 500;
	}
	.btn-sm:hover { background: #0d3d4a; }
	.btn-group { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 8px; }

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
		padding: 14px 18px; margin-top: 12px;
	}
	.security-note > p { font-size: 0.875rem; color: #78350f; margin-bottom: 8px; }
	.subtle-link { color: inherit; text-decoration: underline; }
	.subtle-link:hover { opacity: 0.7; }

	/* Inline link inside body text (for "tray" → troubleshooting jump). */
	.inline-link {
		color: #0f4c5c; text-decoration: underline; text-underline-offset: 2px;
	}
	.inline-link:hover { color: #0d3d4a; }

	/* Wide strip screenshots (menu bar, taskbar) - width-bound. */
	.screenshot-sm {
		display: block; max-width: 280px; width: 100%; height: auto;
		border: 1px solid #e5e7eb; border-radius: 8px;
		margin: 8px 0 12px; background: #fff;
	}
	/* Square / portrait screenshots (e.g. the Windows tray panel) -
	   smaller still so they don't dominate the page. */
	.screenshot-square {
		display: block; max-width: 180px; width: 100%; height: auto;
		border: 1px solid #e5e7eb; border-radius: 8px;
		margin: 8px 0 12px; background: #fff;
	}
	/* Wider menu screenshots (e.g. the Claude Desktop connector toggle) where
	   the small size would make the UI text unreadable. */
	.screenshot-md {
		display: block; max-width: 300px; width: 100%; height: auto;
		border: 1px solid #e5e7eb; border-radius: 8px;
		margin: 8px 0 12px 40px; background: #fff;
	}

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
		.diagram { flex-direction: column; }
		.diagram-arrow { flex-direction: row; min-width: 0; }
		.arrow-line { transform: rotate(90deg); }
		.tabs { flex-wrap: wrap; }
	}
</style>
