<script lang="ts">
	import { onMount, tick } from 'svelte';

	let { data } = $props();
	const p = data.pipeline;

	let showAll = $state(false);
	let selectedFile = $state('');
	let fileContent = $state('');
	let loadingFile = $state(false);
	let fileContentCache: Record<string, string> = {};

	const displayedVersions = $derived(
		showAll ? data.versionChain : data.versionChain.slice(0, 3)
	);

	// Build tree structure from flat file list
	type TreeNode = { name: string; path: string; type: 'file' | 'folder'; children: TreeNode[] };

	function buildTree(files: { path: string; type: string }[]): TreeNode[] {
		const root: TreeNode[] = [];
		const blobs = files.filter(f => f.type === 'blob');

		for (const file of blobs) {
			const parts = file.path.split('/');
			let current = root;

			for (let i = 0; i < parts.length; i++) {
				const name = parts[i];
				const isFile = i === parts.length - 1;
				const existing = current.find(n => n.name === name);

				if (existing) {
					current = existing.children;
				} else {
					const node: TreeNode = {
						name,
						path: isFile ? file.path : parts.slice(0, i + 1).join('/'),
						type: isFile ? 'file' : 'folder',
						children: []
					};
					current.push(node);
					current = node.children;
				}
			}
		}

		// Sort: folders first, then files, alphabetically
		function sortNodes(nodes: TreeNode[]) {
			nodes.sort((a, b) => {
				if (a.type !== b.type) return a.type === 'folder' ? -1 : 1;
				return a.name.localeCompare(b.name);
			});
			nodes.forEach(n => sortNodes(n.children));
		}
		sortNodes(root);
		return root;
	}

	const fileTree = buildTree(data.fileTree.files);

	// Track expanded folders
	let expandedFolders = $state<Set<string>>(new Set(
		// Auto-expand all folders
		data.fileTree.files.filter((f: any) => f.type === 'tree').map((f: any) => f.path)
	));

	function toggleFolder(path: string) {
		const next = new Set(expandedFolders);
		if (next.has(path)) next.delete(path);
		else next.add(path);
		expandedFolders = next;
	}

	// Language detection from filename
	function detectLang(filename: string): string {
		const name = filename.toLowerCase();
		if (name === 'snakefile' || name === 'snakefile.check') return 'python';
		if (name === 'dockerfile') return 'dockerfile';
		if (name.endsWith('.py')) return 'python';
		if (name.endsWith('.r')) return 'r';
		if (name.endsWith('.sh') || name.endsWith('.bash')) return 'bash';
		if (name.endsWith('.yaml') || name.endsWith('.yml')) return 'yaml';
		if (name.endsWith('.json')) return 'json';
		if (name.endsWith('.md')) return 'markdown';
		if (name.endsWith('.toml')) return 'ini';
		if (name.endsWith('.cfg') || name.endsWith('.ini') || name.endsWith('.conf')) return 'ini';
		if (name.endsWith('.js')) return 'javascript';
		if (name.endsWith('.ts')) return 'typescript';
		if (name.endsWith('.txt') || name === '.dockerignore' || name === '.gitignore') return 'plaintext';
		if (name.endsWith('.pl')) return 'perl';
		if (name.endsWith('.java')) return 'java';
		if (name.endsWith('.scala')) return 'scala';
		if (name.endsWith('.go')) return 'go';
		if (name.endsWith('.rs')) return 'rust';
		if (name.endsWith('.cpp') || name.endsWith('.c') || name.endsWith('.h')) return 'cpp';
		return 'plaintext';
	}

	let hljsReady = $state(false);

	onMount(async () => {
		const link = document.createElement('link');
		link.rel = 'stylesheet';
		link.href = 'https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/styles/github.min.css';
		document.head.appendChild(link);
		const script = document.createElement('script');
		script.src = 'https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/highlight.min.js';
		script.onload = () => { hljsReady = true; };
		document.head.appendChild(script);

		// No auto-select — user picks a file from the tree
	});

	async function selectFile(path: string) {
		selectedFile = path;

		if (fileContentCache[path]) {
			fileContent = fileContentCache[path];
			await tick();
			highlightCode();
			return;
		}

		loadingFile = true;
		try {
			const resp = await fetch(`/pipelines/${p.pipeline_id}/file?path=${encodeURIComponent(path)}`);
			const json = await resp.json();
			fileContent = json.content || '';
			fileContentCache[path] = fileContent;
		} catch {
			fileContent = '// Failed to load file';
		}
		loadingFile = false;
		await tick();
		highlightCode();
	}

	function highlightCode() {
		if (typeof (window as any).hljs !== 'undefined') {
			const el = document.querySelector('.code-viewer pre code');
			if (el) {
				(el as HTMLElement).removeAttribute('data-highlighted');
				(window as any).hljs.highlightElement(el);
			}
		}
	}
</script>

<svelte:head>
	<title>{p.name} - Autopipe Hub</title>
</svelte:head>

<main>
	<div class="back-link-wrap"><a href="/" class="back-link">&larr; Back to list</a></div>

	<div class="detail-header">
		<div>
			<h2>{p.name}</h2>
			<p class="detail-desc">{p.description}</p>
			{#if data.basedOnUrl}
				<p class="based-on">Based on: <a href={data.basedOnUrl} target="_blank" rel="noopener">{data.basedOnUrl.includes('workflowhub.eu') ? 'WorkflowHub' : 'External'} workflow</a></p>
			{:else if data.basedOn}
				<p class="based-on">Based on: <a href="/pipelines/{data.basedOn.pipeline_id}">{data.basedOn.name} v{data.basedOn.version}</a> by {data.basedOn.author}</p>
			{/if}
		</div>
		<div style="display:flex;gap:8px">
			<a href={p.github_url} target="_blank" rel="noopener" class="btn" style="background:#24292e">GitHub</a>
			<a href="/pipelines/{p.pipeline_id}/download" class="btn">Download ZIP</a>
		</div>
	</div>

	<div class="detail-info">
		<div class="detail-info-item">
			<span class="label">VERSION</span>
			<span class="value">{p.version}</span>
		</div>
		<div class="detail-info-item">
			<span class="label">AUTHOR</span>
			<span class="value">{p.author || 'unknown'}</span>
		</div>
		<div class="detail-info-item">
			<span class="label">INPUT</span>
			<span class="value">
				{#each p.input_formats as fmt, i}
					<span class="tag">{fmt}</span>
				{/each}
				{#if p.input_formats.length === 0}—{/if}
			</span>
		</div>
		<div class="detail-info-item">
			<span class="label">OUTPUT</span>
			<span class="value">
				{#each p.output_formats as fmt}
					<span class="tag">{fmt}</span>
				{/each}
				{#if p.output_formats.length === 0}—{/if}
			</span>
		</div>
	</div>

	<div class="detail-tags">
		<div class="tag-row">
			<span class="label">TOOLS</span>
			<div class="tag-list">
				{#each p.tools as tool}
					<span class="tag tool">{tool}</span>
				{/each}
				{#if p.tools.length === 0}<span class="tag-empty">—</span>{/if}
			</div>
		</div>
		<div class="tag-row">
			<span class="label">TAGS</span>
			<div class="tag-list">
				{#each p.tags.filter((t: string) => !p.tools.includes(t)) as tag}
					<span class="tag">{tag}</span>
				{/each}
				{#if p.tags.length === 0}<span class="tag-empty">—</span>{/if}
			</div>
		</div>
	</div>

	<div class="detail-layout">
		<div class="detail-main">
			<div class="files-section">
				<!-- File Tree + Code Viewer -->
				<div class="file-tree-panel">
					<div class="file-tree-header">FILES</div>
					<div class="file-tree">
						{#snippet renderTree(nodes: any[], depth: number)}
							{#each nodes as node}
								{#if node.type === 'folder'}
									<button
										class="tree-item folder"
										style="padding-left: {12 + depth * 16}px"
										onclick={() => toggleFolder(node.path)}
									>
										<span class="tree-icon">{expandedFolders.has(node.path) ? '▾' : '▸'}</span>
										<span class="tree-name">{node.name}</span>
									</button>
									{#if expandedFolders.has(node.path)}
										{@render renderTree(node.children, depth + 1)}
									{/if}
								{:else}
									<button
										class="tree-item file"
										class:active={selectedFile === node.path}
										style="padding-left: {12 + depth * 16}px"
										onclick={() => selectFile(node.path)}
									>
										<span class="tree-icon">📄</span>
										<span class="tree-name">{node.name}</span>
									</button>
								{/if}
							{/each}
						{/snippet}
						{@render renderTree(fileTree, 0)}
					</div>
				</div>
				<div class="code-viewer-panel">
					{#if selectedFile}
						<div class="code-viewer-header">
							<span class="code-viewer-filename">{selectedFile}</span>
						</div>
						<div class="code-viewer">
							{#if loadingFile}
								<div class="code-loading">Loading...</div>
							{:else}
								<pre><code class="language-{detectLang(selectedFile)}">{fileContent}</code></pre>
							{/if}
						</div>
					{:else}
						<div class="code-viewer-empty">
							<p>Select a file from the tree to view its contents.</p>
						</div>
					{/if}
				</div>
			</div>
		</div>

		<!-- Version Timeline (right sidebar, outside code block) -->
		<div class="detail-sidebar">
			<div class="sidebar-title">VERSION HISTORY</div>
			<div class="version-timeline">
				<div class="version-line"></div>
				{#each displayedVersions as v, i (v.pipeline_id)}
					{#if i === 0}
						<div class="version-item">
							<div class="version-dot current"></div>
							<div class="version-card current">
								<span class="version-ver">v{v.version}</span>
								<span class="version-badge">latest</span>
								<div class="version-meta">{v.created_at?.split('T')[0] || '—'} · {v.author || 'unknown'}</div>
							</div>
						</div>
					{:else}
						<a href="/pipelines/{v.pipeline_id}/download?tag={encodeURIComponent(v.name + '/v' + v.version)}" class="version-item">
							<div class="version-dot"></div>
							<div class="version-card">
								<span class="version-ver">v{v.version}</span>
								<span class="version-badge download">Download ZIP</span>
								<div class="version-meta">{v.created_at?.split('T')[0] || '—'} · {v.author || 'unknown'}</div>
							</div>
						</a>
					{/if}
				{/each}
				{#if data.versionChain.length > 3 && !showAll}
					<button class="version-more" onclick={() => showAll = true}>show more ({data.versionChain.length - 3} more)</button>
				{/if}
			</div>
		</div>
	</div>
</main>
