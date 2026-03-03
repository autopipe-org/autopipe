<svelte:head>
	<title>Plugin Creation Guide - AutoPipe</title>
</svelte:head>

<main>
	<div class="guide">
		<a href="/plugins" class="back-link">&larr; Back to Plugins</a>

		<h1 class="guide-title">Plugin Creation Guide</h1>
		<p class="guide-intro">
			AutoPipe plugins extend the Results Viewer with custom file previews.
			Plugins are HTML/JavaScript-based and run directly in the browser.
		</p>

		<!-- Section 1: What is a Plugin -->
		<section class="guide-section">
			<h2>1. What is a Plugin?</h2>
			<p>
				The AutoPipe Results Viewer includes built-in viewers for images, PDFs, text files,
				BAM/VCF/BED (via igv.js), and h5ad (via jsfive). Plugins let you add support
				for additional file formats beyond what's built in.
			</p>
			<p>
				For example, if you need a custom visualization for <code>.xyz</code> files,
				you can create a plugin that handles that format.
			</p>
		</section>

		<!-- Section 2: Plugin Structure -->
		<section class="guide-section">
			<h2>2. Plugin Structure</h2>
			<div class="code-block">
				<pre>{`my-viewer-plugin/
├── manifest.json    # Plugin metadata (required)
├── index.js         # Main entry point (required)
├── style.css        # Stylesheet (optional)
└── lib/             # Additional libraries (optional)`}</pre>
			</div>

			<h3>manifest.json</h3>
			<div class="code-block">
				<pre>{`{
  "name": "my-viewer-plugin",
  "version": "1.0.0",
  "description": "Custom viewer for .xyz files",
  "author": "your-github-username",
  "extensions": ["xyz", "abc"],
  "entry": "index.js",
  "style": "style.css"
}`}</pre>
			</div>
			<table class="field-table">
				<thead>
					<tr><th>Field</th><th>Required</th><th>Description</th></tr>
				</thead>
				<tbody>
					<tr><td><code>name</code></td><td>Yes</td><td>Unique plugin name</td></tr>
					<tr><td><code>version</code></td><td>Yes</td><td>Semantic version (e.g., 1.0.0)</td></tr>
					<tr><td><code>description</code></td><td></td><td>Plugin description</td></tr>
					<tr><td><code>extensions</code></td><td>Yes</td><td>Array of supported file extensions</td></tr>
					<tr><td><code>entry</code></td><td>Yes</td><td>Path to main JavaScript file</td></tr>
					<tr><td><code>style</code></td><td></td><td>Path to CSS file (optional)</td></tr>
				</tbody>
			</table>
		</section>

		<!-- Section 3: Plugin JS API -->
		<section class="guide-section">
			<h2>3. Plugin JavaScript API</h2>
			<p>
				In your entry file (<code>index.js</code>), define the <code>window.AutoPipePlugin</code> object:
			</p>
			<div class="code-block">
				<pre>{`window.AutoPipePlugin = {
  // Required: render the file
  render: function(container, fileUrl, filename) {
    // container: DOM element to render into
    // fileUrl: URL to fetch file data (e.g., "/file/result.xyz")
    // filename: the file name (e.g., "result.xyz")

    fetch(fileUrl)
      .then(resp => resp.text())
      .then(data => {
        container.innerHTML = '<pre>' + data + '</pre>';
      });
  },

  // Optional: cleanup when switching to another file
  destroy: function() {
    // Remove event listeners, timers, etc.
  }
};`}</pre>
			</div>
			<p>
				<code>render()</code> is called when the user selects a file with a matching extension.
				Render your content into <code>container</code>.
				Use <code>fetch(fileUrl)</code> to retrieve the file data.
			</p>
		</section>

		<!-- Section 4: Quick Start with CLI -->
		<section class="guide-section">
			<h2>4. Quick Start with CLI</h2>
			<p>
				The easiest way to create a new plugin is with the <code>autopipe-ext</code> CLI tool:
			</p>
			<ol class="step-list">
				<li>
					<strong>Install the CLI tool</strong>
					<div class="code-block">
						<pre>npm install -g @pnucolab/autopipe-ext</pre>
					</div>
				</li>
				<li>
					<strong>Scaffold a new plugin</strong>
					<div class="code-block">
						<pre>autopipe-ext init</pre>
					</div>
					<p>Interactive prompts will ask for the plugin name, description, and supported file extensions.
						It generates <code>manifest.json</code>, <code>index.js</code>, and <code>README.md</code> automatically.</p>
				</li>
			</ol>
			<p>
				Alternatively, you can create the files manually (see Section 2 and 3 above).
			</p>
		</section>

		<!-- Section 5: Development & Testing -->
		<section class="guide-section">
			<h2>5. Development & Testing</h2>
			<ol class="step-list">
				<li>
					<strong>Copy the plugin to the plugins directory</strong>
					<p>
						Default location: <code>~/.local/share/autopipe/plugins/my-plugin/</code>
						(configurable in app settings)
					</p>
				</li>
				<li>
					<strong>Edit <code>index.js</code> with your rendering logic</strong>
				</li>
				<li>
					<strong>Run <code>show_results</code> in AutoPipe</strong>
					<p>Verify that files with the matching extension are rendered by your plugin in the viewer.</p>
				</li>
			</ol>
		</section>

		<!-- Section 6: GitHub Account -->
		<section class="guide-section">
			<h2>6. GitHub Account</h2>
			<p>
				A GitHub account is required to publish plugins to the registry.
				If you don't have one, sign up at <a href="https://github.com/signup" target="_blank" rel="noopener">github.com/signup</a>.
			</p>
		</section>

		<!-- Section 7: GitHub Token -->
		<section class="guide-section">
			<h2>7. GitHub Personal Access Token</h2>
			<ol class="step-list">
				<li>
					Go to <a href="https://github.com/settings/tokens" target="_blank" rel="noopener">GitHub Settings &rarr; Developer settings &rarr; Personal access tokens &rarr; Tokens (classic)</a>
				</li>
				<li>Click <strong>Generate new token (classic)</strong></li>
				<li>
					Configure:
					<ul>
						<li>Note: <code>autopipe-plugin</code></li>
						<li>Expiration: 90 days recommended</li>
						<li>Scopes: check <code>public_repo</code> only</li>
					</ul>
				</li>
				<li>Click <strong>Generate token</strong></li>
				<li>Copy the token starting with <code>ghp_...</code> (shown only once)</li>
			</ol>
			<div class="callout">
				This token is used to verify authorship and access the GitHub repository when publishing.
				The token is not stored on the server.
			</div>
		</section>

		<!-- Section 8: GitHub Repository -->
		<section class="guide-section">
			<h2>8. Create a GitHub Repository</h2>
			<ol class="step-list">
				<li>Create a new public repository on GitHub.</li>
				<li>Push your plugin files:
					<div class="code-block">
						<pre>{`cd my-viewer-plugin
git init
git add .
git commit -m "Initial plugin"
git remote add origin https://github.com/username/my-viewer-plugin.git
git push -u origin main`}</pre>
					</div>
				</li>
			</ol>
		</section>

		<!-- Section 9: Packaging & Publishing -->
		<section class="guide-section">
			<h2>9. Packaging & Publishing</h2>
			<ol class="step-list">
				<li>
					<strong>Validate</strong>
					<div class="code-block">
						<pre>autopipe-ext package</pre>
					</div>
					<p>Checks manifest.json validity, entry file existence, and AutoPipePlugin pattern.</p>
				</li>
				<li>
					<strong>Publish</strong>
					<div class="code-block">
						<pre>{`autopipe-ext publish --token ghp_xxx
# Or via environment variable: GITHUB_TOKEN=ghp_xxx autopipe-ext publish
# Or interactive prompt: autopipe-ext publish`}</pre>
					</div>
					<p>Automatically detects the GitHub URL from git remote and registers the plugin in the registry.</p>
				</li>
			</ol>
		</section>

		<!-- Section 10: Example -->
		<section class="guide-section">
			<h2>10. Example: CSV Heatmap Plugin</h2>
			<h3>manifest.json</h3>
			<div class="code-block">
				<pre>{`{
  "name": "csv-heatmap-viewer",
  "version": "1.0.0",
  "description": "Display CSV data as a color heatmap",
  "extensions": ["csv"],
  "entry": "index.js"
}`}</pre>
			</div>
			<h3>index.js</h3>
			<div class="code-block">
				<pre>{`window.AutoPipePlugin = {
  render: function(container, fileUrl, filename) {
    fetch(fileUrl)
      .then(function(resp) { return resp.text(); })
      .then(function(text) {
        var lines = text.trim().split('\\n');
        var headers = lines[0].split(',');
        var html = '<table style="border-collapse:collapse;font-size:12px;">';
        html += '<tr>' + headers.map(function(h) {
          return '<th style="padding:4px 8px;border:1px solid #ddd;">' + h + '</th>';
        }).join('') + '</tr>';
        for (var i = 1; i < Math.min(lines.length, 100); i++) {
          var cells = lines[i].split(',');
          html += '<tr>' + cells.map(function(c) {
            var n = parseFloat(c);
            var bg = isNaN(n) ? '#fff' :
              'hsl(' + Math.max(0, Math.min(240, 240 - n * 2.4)) + ',70%,85%)';
            return '<td style="padding:4px 8px;border:1px solid #eee;background:' + bg + '">' + c + '</td>';
          }).join('') + '</tr>';
        }
        html += '</table>';
        container.innerHTML = html;
      });
  }
};`}</pre>
			</div>
		</section>
	</div>
</main>

<style>
	.guide {
		max-width: 800px;
		margin: 0 auto;
		padding: 24px;
	}
	.back-link {
		display: inline-block;
		margin-bottom: 16px;
		color: #666;
		text-decoration: none;
		font-size: 14px;
	}
	.back-link:hover {
		color: #0366d6;
	}
	.guide-title {
		font-size: 28px;
		font-weight: 700;
		margin-bottom: 8px;
		color: #111;
	}
	.guide-intro {
		font-size: 15px;
		color: #555;
		line-height: 1.7;
		margin-bottom: 32px;
	}
	.guide-section {
		margin-bottom: 36px;
	}
	.guide-section h2 {
		font-size: 20px;
		font-weight: 700;
		color: #111;
		margin-bottom: 12px;
		padding-bottom: 8px;
		border-bottom: 1px solid #eee;
	}
	.guide-section h3 {
		font-size: 15px;
		font-weight: 600;
		color: #333;
		margin: 16px 0 8px;
	}
	.guide-section p {
		font-size: 14px;
		color: #444;
		line-height: 1.7;
		margin-bottom: 8px;
	}
	.guide-section a {
		color: #0366d6;
	}
	.code-block {
		background: #f6f8fa;
		border: 1px solid #e5e5e5;
		border-radius: 8px;
		padding: 14px 16px;
		margin: 8px 0 12px;
		overflow-x: auto;
	}
	.code-block pre {
		font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace;
		font-size: 13px;
		line-height: 1.5;
		color: #24292f;
		white-space: pre;
		margin: 0;
	}
	.field-table {
		width: 100%;
		border-collapse: collapse;
		font-size: 13px;
		margin: 12px 0;
	}
	.field-table th {
		background: #f6f8fa;
		padding: 8px 12px;
		text-align: left;
		font-weight: 600;
		border-bottom: 2px solid #e5e5e5;
	}
	.field-table td {
		padding: 6px 12px;
		border-bottom: 1px solid #f0f0f0;
	}
	.field-table code {
		background: #eff1f3;
		padding: 2px 6px;
		border-radius: 4px;
		font-size: 12px;
	}
	.step-list {
		padding-left: 24px;
		margin: 8px 0;
	}
	.step-list li {
		font-size: 14px;
		color: #444;
		line-height: 1.7;
		margin-bottom: 12px;
	}
	.step-list li strong {
		color: #111;
	}
	.step-list li p {
		margin: 4px 0 0;
		font-size: 13px;
		color: #666;
	}
	.step-list ul {
		padding-left: 20px;
		margin: 4px 0;
	}
	.step-list ul li {
		margin-bottom: 2px;
		font-size: 13px;
	}
	.callout {
		background: #fff8e1;
		border: 1px solid #ffe082;
		border-radius: 8px;
		padding: 12px 16px;
		font-size: 13px;
		color: #5d4037;
		margin: 12px 0;
		line-height: 1.6;
	}
</style>
