<script lang="ts">
	import { env } from '$env/dynamic/public';
	let { children } = $props();
	const hubUrl = env.PUBLIC_HUB_URL;
	let menuOpen = $state(false);
</script>

<svelte:head>
	<link rel="preconnect" href="https://fonts.googleapis.com">
	<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="anonymous">
	<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet">
</svelte:head>

<div class="plugin-layout">
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
				<a href="/plugins" class="active" onclick={() => menuOpen = false}>Plugins</a>
				<a href="/getting-started" onclick={() => menuOpen = false}>Getting Started</a>
			</div>
		</nav>
	</header>
	{@render children()}
	<footer>
		<div class="footer-content">
			<a href="/" class="footer-logo">
				<img src="/logo.png" alt="Autopipe" />
				<span>Autopipe</span>
			</a>
			<span class="footer-copy">&copy; 2026 Autopipe</span>
		</div>
	</footer>
</div>

<style>
	:global(*) { margin: 0; padding: 0; box-sizing: border-box; }
	:global(body) { font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: #fafafa; color: #111; line-height: 1.5; font-size: 15px; }

	header { position: sticky; top: 0; background: #fff; border-bottom: 1px solid #eee; z-index: 100; }
	nav { max-width: 1200px; margin: 0 auto; padding: 16px 24px; display: flex; align-items: center; justify-content: space-between; }
	.logo { display: flex; align-items: center; gap: 10px; text-decoration: none; color: #111; font-weight: 700; font-size: 1.25rem; }
	.logo img { height: 32px; width: auto; }
	.nav-links { display: flex; gap: 32px; }
	.nav-links a { text-decoration: none; color: #4b5563; font-weight: 500; font-size: 0.95rem; transition: color 0.2s; }
	.nav-links a:hover, .nav-links a.active { color: #111; }

	.hamburger { display: none; background: none; border: none; cursor: pointer; padding: 4px; flex-direction: column; gap: 5px; }
	.hamburger span { display: block; width: 24px; height: 2px; background: #111; transition: transform 0.3s, opacity 0.3s; }
	.hamburger.open span:nth-child(1) { transform: translateY(7px) rotate(45deg); }
	.hamburger.open span:nth-child(2) { opacity: 0; }
	.hamburger.open span:nth-child(3) { transform: translateY(-7px) rotate(-45deg); }

	:global(main) { max-width: 1200px; margin: 0 auto; padding: 32px 24px; }

	footer { border-top: 1px solid #e5e7eb; padding: 24px; margin-top: 48px; }
	.footer-content { max-width: 1200px; margin: 0 auto; display: flex; align-items: center; justify-content: space-between; }
	.footer-logo { display: flex; align-items: center; gap: 8px; font-weight: 700; text-decoration: none; color: #111; }
	.footer-logo img { height: 20px; }
	.footer-copy { color: #9ca3af; font-size: 0.8rem; }

	/* Shared plugin styles */
	:global(.section) { margin-bottom: 28px; }
	:global(.section-title) { font-size: 16px; font-weight: 600; color: #333; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 2px solid #e5e5e5; }
	:global(.section-count) { font-weight: 400; color: #999; font-size: 13px; }
	:global(.search input) { width: 100%; padding: 11px 16px; border: 1px solid #ddd; border-radius: 8px; font-size: 15px; background: #fff; transition: border-color 0.2s; outline: none; }
	:global(.search input:focus) { border-color: #999; }
	:global(.pagination) { display: flex; justify-content: center; align-items: center; gap: 4px; margin-top: 20px; padding: 16px 0; }
	:global(.page-btn) { min-width: 36px; height: 36px; padding: 0 10px; border: 1px solid #ddd; border-radius: 8px; background: #fff; color: #555; font-size: 13px; font-weight: 500; cursor: pointer; transition: all 0.15s; }
	:global(.page-btn:hover) { background: #f0f0f0; border-color: #ccc; }
	:global(.page-btn.active) { background: #111; color: #fff; border-color: #111; }
	:global(.empty) { text-align: center; color: #999; padding: 60px 20px; font-size: 14px; grid-column: 1 / -1; }

	:global(.plugin-guide) { background: #f6f8fa; border: 1px solid #e5e5e5; border-radius: 10px; padding: 16px 20px; margin-bottom: 24px; }
	:global(.plugin-guide p) { font-size: 13px; color: #555; line-height: 1.6; margin: 0; }
	:global(.guide-link) { display: inline-block; margin-top: 8px; font-size: 13px; color: #0366d6; text-decoration: none; font-weight: 500; }
	:global(.guide-link:hover) { text-decoration: underline; }

	:global(.plugin-grid) { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; }
	@media (max-width: 1024px) { :global(.plugin-grid) { grid-template-columns: repeat(2, 1fr); } }
	@media (max-width: 640px) { :global(.plugin-grid) { grid-template-columns: 1fr; } }
	:global(.plugin-card) { display: flex; flex-direction: column; background: #fff; border: 1px solid #e5e5e5; border-radius: 10px; padding: 20px; text-decoration: none; color: inherit; transition: border-color 0.15s, box-shadow 0.15s; }
	:global(.plugin-card:hover) { border-color: #ccc; box-shadow: 0 2px 8px rgba(0,0,0,0.06); }
	:global(.plugin-card-header) { display: flex; align-items: center; gap: 12px; margin-bottom: 12px; }
	:global(.plugin-card-icon) { width: 40px; height: 40px; border-radius: 8px; background: #f0f0f0; display: flex; align-items: center; justify-content: center; font-size: 18px; font-weight: 700; color: #555; flex-shrink: 0; }
	:global(.plugin-card-meta) { min-width: 0; }
	:global(.plugin-card-name) { font-size: 15px; font-weight: 600; color: #111; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
	:global(.plugin-card-version) { font-size: 12px; color: #999; }
	:global(.plugin-card-desc) { font-size: 13px; color: #666; line-height: 1.5; margin-bottom: 12px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
	:global(.plugin-card-exts) { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 10px; margin-top: auto; }
	:global(.plugin-ext-tag) { display: inline-block; padding: 2px 8px; border-radius: 4px; font-size: 11px; background: #f0f0f0; color: #555; font-family: 'SF Mono', 'Consolas', monospace; }
	:global(.plugin-card-footer) { display: flex; flex-wrap: wrap; gap: 6px; }
	:global(.plugin-card-author) { font-size: 12px; color: #999; }

	/* Detail styles */
	:global(.back-link-wrap) { margin-bottom: 24px; }
	:global(.back-link) { font-size: 13px; color: #888; text-decoration: none; }
	:global(.back-link:hover) { color: #111; }
	:global(.detail-layout) { display: flex; gap: 32px; }
	:global(.detail-main) { flex: 3; min-width: 0; }
	:global(.detail-sidebar) { flex: 1; min-width: 240px; max-width: 300px; }
	:global(.detail-header) { display: flex; justify-content: space-between; align-items: flex-start; gap: 24px; margin-bottom: 24px; }
	:global(.detail-header h2) { font-size: 1.5rem; font-weight: 700; margin-bottom: 8px; }
	:global(.detail-desc) { color: #666; font-size: 14px; }
	:global(.detail-info) { display: flex; gap: 48px; padding: 20px 0; border-top: 1px solid #eee; border-bottom: 1px solid #eee; margin-bottom: 16px; }
	:global(.detail-info-item) { display: flex; flex-direction: column; gap: 4px; }
	:global(.label) { font-size: 11px; font-weight: 600; color: #999; letter-spacing: 0.04em; }
	:global(.value) { font-size: 14px; color: #111; }
	:global(.btn) { display: inline-block; padding: 9px 22px; background: #111; color: #fff; text-decoration: none; border-radius: 8px; font-size: 13px; font-weight: 500; transition: background 0.2s; white-space: nowrap; }
	:global(.btn:hover) { background: #333; }

	/* Version timeline */
	:global(.sidebar-title) { font-size: 11px; font-weight: 600; color: #999; letter-spacing: 0.04em; margin-bottom: 16px; }
	:global(.version-timeline) { position: relative; padding-left: 28px; }
	:global(.version-line) { position: absolute; left: 8px; top: 12px; bottom: 12px; width: 2px; background: #e5e5e5; }
	:global(.version-item) { position: relative; margin-bottom: 24px; cursor: pointer; text-decoration: none; display: block; color: inherit; }
	:global(.version-item:last-child) { margin-bottom: 0; }
	:global(.version-dot) { position: absolute; left: -24px; top: 6px; width: 12px; height: 12px; border-radius: 50%; border: 2px solid #ddd; background: #fff; }
	:global(.version-dot.current) { background: #111; border-color: #111; }
	:global(.version-card) { padding: 16px; border: 1px solid transparent; border-radius: 8px; }
	:global(.version-card.current) { background: #f8f8f8; border-color: #eee; }
	:global(.version-view-more) { background: none; border: none; color: #666; font-size: 12px; cursor: pointer; padding: 4px 0; margin-top: 4px; }
	:global(.version-view-more:hover) { color: #111; }
	:global(.version-ver) { font-family: 'SF Mono', 'Consolas', monospace; font-size: 15px; font-weight: 600; color: #111; }
	:global(.version-badge) { display: inline-block; font-size: 10px; color: #999; border: 1px solid #ddd; border-radius: 100px; padding: 1px 8px; margin-left: 8px; }
	:global(.version-meta) { font-size: 12px; color: #999; margin-top: 4px; }

	/* README */
	:global(.readme-section) { background: #fff; border: 1px solid #e5e5e5; border-radius: 10px; padding: 24px 28px; margin-top: 16px; }
	:global(.readme-content) { font-size: 14px; line-height: 1.7; color: #333; }
	:global(.readme-content h1) { font-size: 1.4em; font-weight: 700; margin: 20px 0 10px; padding-bottom: 6px; border-bottom: 1px solid #eee; }
	:global(.readme-content h2) { font-size: 1.2em; font-weight: 600; margin: 18px 0 8px; }
	:global(.readme-content p) { margin: 8px 0; }
	:global(.readme-content code) { font-family: 'SF Mono','Consolas',monospace; font-size: 0.9em; background: #f5f5f5; padding: 2px 6px; border-radius: 4px; }
	:global(.readme-content pre) { background: #f5f5f5; padding: 14px 18px; border-radius: 8px; overflow-x: auto; margin: 12px 0; }
	:global(.readme-content pre code) { background: none; padding: 0; font-size: 13px; }
	:global(.readme-content ul, .readme-content ol) { padding-left: 24px; margin: 8px 0; }
	:global(.readme-content a) { color: #0366d6; }
	:global(.readme-empty) { color: #999; font-size: 14px; }

	@media (max-width: 768px) {
		.hamburger { display: flex; }
		.nav-links { display: none; position: absolute; top: 100%; left: 0; right: 0; background: #fff; flex-direction: column; padding: 16px 24px; gap: 16px; border-bottom: 1px solid #eee; box-shadow: 0 4px 12px rgba(0,0,0,0.08); }
		.nav-links.open { display: flex; }
		nav { position: relative; }
		:global(.detail-layout) { flex-direction: column; }
		:global(.detail-sidebar) { max-width: 100%; }
		:global(.detail-info) { flex-wrap: wrap; gap: 16px; }
		.footer-content { flex-direction: column; gap: 8px; }
	}
</style>
