export const CSS = `
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Inter', sans-serif; background: #fafafa; color: #111; line-height: 1.5; font-size: 15px; }

/* Header */
header { padding: 14px 40px 0; border-bottom: 1px solid #eee; background: #fff; }
.header-top { display: flex; align-items: baseline; gap: 12px; margin-bottom: 12px; }
.header-sub { font-size: 14px; color: #999; font-weight: 400; }
.header-tabs { display: flex; gap: 24px; }
.header-tab { font-size: 15px; font-weight: 500; color: #999; text-decoration: none; padding-bottom: 10px; border-bottom: 2px solid transparent; transition: color 0.15s, border-color 0.15s; }
.header-tab:hover { color: #111; }
.header-tab.active { color: #111; font-weight: 600; border-bottom-color: #111; }
.logo { font-size: 1.15rem; font-weight: 700; color: #111; text-decoration: none; letter-spacing: -0.02em; display: flex; align-items: center; gap: 8px; }
.logo-icon { height: 24px; width: auto; }

/* Main layout - full width */
main { padding: 32px 48px; }

/* Sections */
.section { margin-bottom: 28px; }
.section-title { font-size: 16px; font-weight: 600; color: #333; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 2px solid #e5e5e5; }
.section-count { font-weight: 400; color: #999; font-size: 13px; }

/* Search */
.search input { width: 100%; padding: 11px 16px; border: 1px solid #ddd; border-radius: 8px; font-size: 15px; background: #fff; transition: border-color 0.2s; outline: none; }
.search input:focus { border-color: #999; }

/* Pagination */
.pagination { display: flex; justify-content: center; align-items: center; gap: 4px; margin-top: 20px; padding: 16px 0; }
.page-btn { min-width: 36px; height: 36px; padding: 0 10px; border: 1px solid #ddd; border-radius: 8px; background: #fff; color: #555; font-size: 13px; font-weight: 500; cursor: pointer; transition: all 0.15s; }
.page-btn:hover { background: #f0f0f0; border-color: #ccc; }
.page-btn.active { background: #111; color: #fff; border-color: #111; }

/* Pipeline cards */
.grid { display: flex; flex-direction: column; gap: 1px; background: #e5e5e5; border: 1px solid #e5e5e5; border-radius: 10px; overflow: hidden; }
.card { display: block; background: #fff; padding: 24px 28px; text-decoration: none; color: inherit; transition: background 0.15s; }
.card:hover { background: #f8f8f8; }
.card-title { font-size: 16px; font-weight: 600; color: #111; margin-bottom: 12px; }
.card-version { font-weight: 400; color: #aaa; font-size: 13px; margin-left: 8px; }
.card-desc { color: #666; font-size: 14px; margin-bottom: 14px; line-height: 1.5; }
.card-tags { display: flex; flex-wrap: wrap; gap: 6px; }
.tag { display: inline-block; padding: 3px 10px; border-radius: 100px; font-size: 11px; background: #f0f0f0; color: #666; font-weight: 500; }
.tag.tool { background: #111; color: #fff; }
.empty { text-align: center; color: #999; padding: 60px 20px; font-size: 14px; grid-column: 1 / -1; }
.grid:has(.empty) { background: none; border: none; }
.plugin-grid:has(.empty) { display: block; }

/* List layout with filter sidebar */
.list-layout { display: flex; gap: 32px; }
.filter-sidebar { width: 240px; flex-shrink: 0; position: sticky; top: 24px; align-self: flex-start; }
.filter-box { background: #fff; border: 1px solid #e5e5e5; border-radius: 10px; padding: 20px; }
.filter-title { font-size: 11px; font-weight: 600; color: #999; letter-spacing: 0.04em; margin-bottom: 12px; }
.filter-title:not(:first-child) { margin-top: 20px; }
.filter-search { width: 100%; padding: 6px 10px; border: 1px solid #e5e5e5; border-radius: 6px; font-size: 12px; outline: none; margin-bottom: 10px; background: #fafafa; transition: border-color 0.2s; }
.filter-search:focus { border-color: #999; background: #fff; }
.filter-group { display: flex; flex-direction: column; gap: 6px; max-height: 200px; overflow-y: auto; }
.filter-item { display: flex; align-items: center; gap: 8px; cursor: pointer; font-size: 13px; color: #444; padding: 3px 0; }
.filter-item input[type="checkbox"] { accent-color: #111; width: 14px; height: 14px; cursor: pointer; }
.filter-item:hover { color: #111; }
.filter-item .filter-count { color: #bbb; font-size: 11px; margin-left: auto; }
.filter-clear { font-size: 12px; color: #888; cursor: pointer; background: none; border: none; padding: 4px 0; margin-top: 8px; }
.filter-clear:hover { color: #111; }
.list-content { flex: 1; min-width: 0; }

/* Detail page */
.back-link-wrap { margin-bottom: 24px; }
.back-link { font-size: 13px; color: #888; text-decoration: none; }
.back-link:hover { color: #111; }
.detail-header { display: flex; justify-content: space-between; align-items: flex-start; gap: 24px; margin-bottom: 24px; }
.detail-header h2 { font-size: 1.5rem; font-weight: 700; letter-spacing: -0.02em; margin-bottom: 8px; }
.detail-desc { color: #666; font-size: 14px; }
.based-on { font-size: 13px; color: #888; margin-top: 6px; }
.based-on a { color: #0366d6; text-decoration: none; }
.based-on a:hover { text-decoration: underline; }
.detail-info { display: flex; gap: 48px; padding: 20px 0; border-top: 1px solid #eee; border-bottom: 1px solid #eee; margin-bottom: 16px; }
.detail-info-item { display: flex; flex-direction: column; gap: 4px; }
.label { font-size: 11px; font-weight: 600; color: #999; letter-spacing: 0.04em; }
.value { font-size: 14px; color: #111; }
.detail-tags { display: flex; flex-direction: column; gap: 12px; padding: 16px 0; margin-bottom: 24px; }
.tag-row { display: flex; align-items: baseline; gap: 12px; }
.tag-row .label { min-width: 48px; flex-shrink: 0; }
.tag-list { display: flex; flex-wrap: wrap; gap: 6px; }
.tag-empty { font-size: 13px; color: #ccc; }
.btn { display: inline-block; padding: 9px 22px; background: #111; color: #fff; text-decoration: none; border-radius: 8px; font-size: 13px; font-weight: 500; transition: background 0.2s; white-space: nowrap; }
.btn:hover { background: #333; }

/* File tabs */
.files-section { background: #fff; border: 1px solid #e5e5e5; border-radius: 10px; overflow: hidden; }
.tab-bar { display: flex; border-bottom: 1px solid #e5e5e5; background: #fafafa; overflow-x: auto; }
.tab-btn { padding: 10px 20px; border: none; background: none; font-size: 14px; font-weight: 500; color: #888; cursor: pointer; border-bottom: 2px solid transparent; transition: color 0.15s, border-color 0.15s; white-space: nowrap; }
.tab-btn:hover { color: #111; }
.tab-btn.active { color: #111; border-bottom-color: #111; }
.tab-panel { display: none; }
.tab-panel.active { display: block; }
.tab-panel pre { padding: 24px; overflow-x: auto; font-size: 14px; line-height: 1.6; background: #fff; margin: 0; }
.tab-panel pre code { font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace; background: transparent; }

/* Splash screen */
.splash { position: fixed; inset: 0; background: #fff; z-index: 1000; display: flex; align-items: center; justify-content: center; transition: opacity 0.5s; }
.splash-fade { opacity: 0; }
.splash-inner { text-align: center; }
.splash-icon { display: flex; align-items: center; justify-content: center; gap: 0; margin-bottom: 24px; }
.splash-icon .dot { width: 10px; height: 10px; background: #111; border-radius: 50%; }
.splash-icon .line { width: 32px; height: 2px; background: #ccc; }
.splash-title { font-family: 'Georgia', 'Times New Roman', serif; font-size: 2.5rem; font-weight: 700; color: #111; letter-spacing: -0.02em; margin-bottom: 8px; }
.splash-sub { font-size: 14px; color: #999; margin-bottom: 40px; }
.splash-bar { width: 200px; height: 3px; background: #eee; border-radius: 2px; margin: 0 auto 20px; overflow: hidden; }
.splash-bar-fill { width: 0; height: 100%; background: #111; border-radius: 2px; animation: splash-progress 1.2s ease-out forwards; }
@keyframes splash-progress { 0% { width: 0; } 100% { width: 100%; } }
.splash-loading { font-size: 13px; color: #bbb; }
.app-hidden { display: none; }

/* 2-column detail layout */
.detail-layout { display: flex; gap: 32px; }
.detail-main { flex: 3; min-width: 0; }
.detail-sidebar { flex: 1; min-width: 240px; max-width: 300px; }

/* Version timeline */
.sidebar-title { font-size: 11px; font-weight: 600; color: #999; letter-spacing: 0.04em; margin-bottom: 16px; }
.version-timeline { position: relative; padding-left: 28px; }
.version-line { position: absolute; left: 8px; top: 12px; bottom: 12px; width: 2px; background: #e5e5e5; }
.version-item { position: relative; margin-bottom: 24px; cursor: pointer; text-decoration: none; display: block; color: inherit; }
.version-item:last-child { margin-bottom: 0; }
.version-dot { position: absolute; left: -24px; top: 6px; width: 12px; height: 12px; border-radius: 50%; border: 2px solid #ddd; background: #fff; }
.version-dot.current { background: #111; border-color: #111; }
.version-card { padding: 16px; border: 1px solid transparent; border-radius: 8px; }
.version-card.current { background: #f8f8f8; border-color: #eee; }
.version-ver { font-family: 'SF Mono', 'Consolas', monospace; font-size: 15px; font-weight: 600; color: #111; }
.version-badge { display: inline-block; font-size: 10px; color: #999; border: 1px solid #ddd; border-radius: 100px; padding: 1px 8px; margin-left: 8px; vertical-align: middle; }
.version-badge.download { color: #888; border-color: #ccc; cursor: pointer; }
.version-meta { font-size: 12px; color: #999; margin-top: 4px; }
.version-desc { font-size: 13px; color: #888; margin-top: 6px; line-height: 1.4; }
.version-more { font-size: 13px; color: #888; cursor: pointer; background: none; border: none; padding: 8px 0; margin-top: 8px; }
.version-more:hover { color: #111; }

/* Plugin guide banner */
.plugin-guide { background: #f6f8fa; border: 1px solid #e5e5e5; border-radius: 10px; padding: 16px 20px; margin-bottom: 24px; }
.plugin-guide p { font-size: 13px; color: #555; line-height: 1.6; margin: 0; }

/* Plugin card grid */
.plugin-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; }
@media (max-width: 1024px) { .plugin-grid { grid-template-columns: repeat(2, 1fr); } }
@media (max-width: 640px) { .plugin-grid { grid-template-columns: 1fr; } }
.plugin-card { display: flex; flex-direction: column; background: #fff; border: 1px solid #e5e5e5; border-radius: 10px; padding: 20px; text-decoration: none; color: inherit; transition: border-color 0.15s, box-shadow 0.15s; }
.plugin-card:hover { border-color: #ccc; box-shadow: 0 2px 8px rgba(0,0,0,0.06); }
.plugin-card-header { display: flex; align-items: center; gap: 12px; margin-bottom: 12px; }
.plugin-card-icon { width: 40px; height: 40px; border-radius: 8px; background: #f0f0f0; display: flex; align-items: center; justify-content: center; font-size: 18px; font-weight: 700; color: #555; flex-shrink: 0; }
.plugin-card-meta { min-width: 0; }
.plugin-card-name { font-size: 15px; font-weight: 600; color: #111; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.plugin-card-version { font-size: 12px; color: #999; }
.plugin-card-desc { font-size: 13px; color: #666; line-height: 1.5; margin-bottom: 12px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
.plugin-card-exts { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 10px; margin-top: auto; }
.plugin-ext-tag { display: inline-block; padding: 2px 8px; border-radius: 4px; font-size: 11px; background: #f0f0f0; color: #555; font-family: 'SF Mono', 'Consolas', monospace; }
.plugin-card-footer { display: flex; flex-wrap: wrap; gap: 6px; }
.plugin-card-author { font-size: 12px; color: #999; }
.guide-link { display: inline-block; margin-top: 8px; font-size: 13px; color: #0366d6; text-decoration: none; font-weight: 500; }
.guide-link:hover { text-decoration: underline; }

/* README section */
.readme-section { background: #fff; border: 1px solid #e5e5e5; border-radius: 10px; padding: 24px 28px; margin-top: 16px; }
.readme-content { font-size: 14px; line-height: 1.7; color: #333; }
.readme-content h1 { font-size: 1.4em; font-weight: 700; margin: 20px 0 10px; padding-bottom: 6px; border-bottom: 1px solid #eee; }
.readme-content h2 { font-size: 1.2em; font-weight: 600; margin: 18px 0 8px; }
.readme-content h3 { font-size: 1.05em; font-weight: 600; margin: 14px 0 6px; }
.readme-content p { margin: 8px 0; }
.readme-content code { font-family: 'SF Mono','Consolas',monospace; font-size: 0.9em; background: #f5f5f5; padding: 2px 6px; border-radius: 4px; }
.readme-content pre { background: #f5f5f5; padding: 14px 18px; border-radius: 8px; overflow-x: auto; margin: 12px 0; }
.readme-content pre code { background: none; padding: 0; font-size: 13px; }
.readme-content ul, .readme-content ol { padding-left: 24px; margin: 8px 0; }
.readme-content li { margin: 4px 0; }
.readme-content a { color: #0366d6; }
.readme-empty { color: #999; font-size: 14px; }

/* General */
a { color: #111; }
a:hover { color: #555; }
`;
