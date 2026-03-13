export const CSS = `
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: #fff; color: #1a2332; line-height: 1.6; font-size: 15px; }

/* Header */
header { position: sticky; top: 0; background: #fff; border-bottom: 1px solid #e5e7eb; z-index: 100; }
nav { max-width: 1200px; margin: 0 auto; padding: 16px 24px; display: flex; align-items: center; justify-content: space-between; }
.logo { display: flex; align-items: center; gap: 10px; text-decoration: none; color: #1a2332; font-weight: 700; font-size: 1.25rem; }
.logo img { height: 32px; width: auto; }
.nav-links { display: flex; gap: 32px; }
.nav-links a { text-decoration: none; color: #4b5563; font-weight: 500; font-size: 0.95rem; transition: color 0.2s; }
.nav-links a:hover { color: #1a2332; }

/* Footer */
footer { border-top: 1px solid #e5e7eb; padding: 24px; }
.footer-content { max-width: 1200px; margin: 0 auto; display: flex; align-items: center; justify-content: space-between; }
.footer-logo { display: flex; align-items: center; gap: 8px; font-weight: 700; text-decoration: none; color: #1a2332; }
.footer-logo img { height: 20px; }
.footer-copy { color: #9ca3af; font-size: 0.8rem; }

/* Main layout */
main { max-width: 1200px; margin: 0 auto; padding: 32px 24px; }

/* Sections */
.section { margin-bottom: 28px; }
.section-title { font-size: 16px; font-weight: 600; color: #1a2332; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 2px solid #e5e7eb; }
.section-count { font-weight: 400; color: #9ca3af; font-size: 13px; }

/* Search */
.search input { width: 100%; padding: 11px 16px; border: 1px solid #e5e7eb; border-radius: 8px; font-size: 15px; background: #fff; transition: border-color 0.2s; outline: none; }
.search input:focus { border-color: #0f4c5c; }

/* Pagination */
.pagination { display: flex; justify-content: center; align-items: center; gap: 4px; margin-top: 20px; padding: 16px 0; }
.page-btn { min-width: 36px; height: 36px; padding: 0 10px; border: 1px solid #e5e7eb; border-radius: 8px; background: #fff; color: #4b5563; font-size: 13px; font-weight: 500; cursor: pointer; transition: all 0.15s; }
.page-btn:hover { background: #f9fafb; border-color: #d1d5db; }
.page-btn.active { background: #0f4c5c; color: #fff; border-color: #0f4c5c; }

/* Pipeline cards */
.grid { display: flex; flex-direction: column; gap: 1px; background: #e5e7eb; border: 1px solid #e5e7eb; border-radius: 10px; overflow: hidden; }
.card { display: block; background: #fff; padding: 24px 28px; text-decoration: none; color: inherit; transition: background 0.15s; }
.card:hover { background: #f9fafb; }
.card-title { font-size: 16px; font-weight: 600; color: #1a2332; margin-bottom: 12px; }
.card-version { font-weight: 400; color: #9ca3af; font-size: 13px; margin-left: 8px; }
.card-desc { color: #4b5563; font-size: 14px; margin-bottom: 14px; line-height: 1.5; }
.card-tags { display: flex; flex-wrap: wrap; gap: 6px; }
.tag { display: inline-block; padding: 3px 10px; border-radius: 100px; font-size: 11px; background: #f3f4f6; color: #4b5563; font-weight: 500; }
.tag.tool { background: #0f4c5c; color: #fff; }
.empty { text-align: center; color: #9ca3af; padding: 60px 20px; font-size: 14px; grid-column: 1 / -1; }
.grid:has(.empty) { background: none; border: none; }

/* List layout with filter sidebar */
.list-layout { display: flex; gap: 32px; }
.filter-sidebar { width: 240px; flex-shrink: 0; position: sticky; top: 80px; align-self: flex-start; }
.filter-box { background: #fff; border: 1px solid #e5e7eb; border-radius: 10px; padding: 20px; }
.filter-title { font-size: 11px; font-weight: 600; color: #9ca3af; letter-spacing: 0.04em; margin-bottom: 12px; }
.filter-title:not(:first-child) { margin-top: 20px; }
.filter-search { width: 100%; padding: 6px 10px; border: 1px solid #e5e7eb; border-radius: 6px; font-size: 12px; outline: none; margin-bottom: 10px; background: #f9fafb; transition: border-color 0.2s; }
.filter-search:focus { border-color: #0f4c5c; background: #fff; }
.filter-group { display: flex; flex-direction: column; gap: 6px; max-height: 200px; overflow-y: auto; }
.filter-item { display: flex; align-items: center; gap: 8px; cursor: pointer; font-size: 13px; color: #4b5563; padding: 3px 0; }
.filter-item input[type="checkbox"] { accent-color: #0f4c5c; width: 14px; height: 14px; cursor: pointer; }
.filter-item:hover { color: #1a2332; }
.filter-item .filter-count { color: #d1d5db; font-size: 11px; margin-left: auto; }
.filter-clear { font-size: 12px; color: #6b7280; cursor: pointer; background: none; border: none; padding: 4px 0; margin-top: 8px; }
.filter-clear:hover { color: #1a2332; }
.list-content { flex: 1; min-width: 0; }

/* Detail page */
.back-link-wrap { margin-bottom: 24px; }
.back-link { font-size: 13px; color: #6b7280; text-decoration: none; }
.back-link:hover { color: #1a2332; }
.detail-header { display: flex; justify-content: space-between; align-items: flex-start; gap: 24px; margin-bottom: 24px; }
.detail-header h2 { font-size: 1.5rem; font-weight: 700; letter-spacing: -0.02em; margin-bottom: 8px; }
.detail-desc { color: #4b5563; font-size: 14px; }
.based-on { font-size: 13px; color: #6b7280; margin-top: 6px; }
.based-on a { color: #0f4c5c; text-decoration: none; }
.based-on a:hover { text-decoration: underline; }
.detail-info { display: flex; gap: 48px; padding: 20px 0; border-top: 1px solid #e5e7eb; border-bottom: 1px solid #e5e7eb; margin-bottom: 16px; }
.detail-info-item { display: flex; flex-direction: column; gap: 4px; }
.label { font-size: 11px; font-weight: 600; color: #9ca3af; letter-spacing: 0.04em; }
.value { font-size: 14px; color: #1a2332; }
.detail-tags { display: flex; flex-direction: column; gap: 12px; padding: 16px 0; margin-bottom: 24px; }
.tag-row { display: flex; align-items: baseline; gap: 12px; }
.tag-row .label { min-width: 48px; flex-shrink: 0; }
.tag-list { display: flex; flex-wrap: wrap; gap: 6px; }
.tag-empty { font-size: 13px; color: #d1d5db; }
.btn { display: inline-block; padding: 9px 22px; background: #0f4c5c; color: #fff; text-decoration: none; border-radius: 8px; font-size: 13px; font-weight: 500; transition: background 0.2s; white-space: nowrap; }
.btn:hover { background: #0d3d4a; }

/* Files section: file tree + code viewer side by side */
.files-section { display: flex; background: #fff; border: 1px solid #e5e7eb; border-radius: 10px; overflow: hidden; min-height: 500px; }

/* File tree panel */
.file-tree-panel { width: 220px; flex-shrink: 0; background: #f9fafb; border-right: 1px solid #e5e7eb; display: flex; flex-direction: column; }
.file-tree-header { font-size: 11px; font-weight: 600; color: #9ca3af; letter-spacing: 0.04em; padding: 14px 16px 10px; }
.file-tree { overflow-y: auto; flex: 1; padding-bottom: 12px; }
.tree-item { display: flex; align-items: center; gap: 6px; width: 100%; border: none; background: none; padding: 5px 12px; font-size: 13px; color: #4b5563; cursor: pointer; text-align: left; transition: background 0.1s; white-space: nowrap; }
.tree-item:hover { background: #e5e7eb; }
.tree-item.active { background: #dbeafe; color: #1a2332; font-weight: 500; }
.tree-item .tree-icon { font-size: 11px; flex-shrink: 0; width: 16px; text-align: center; }
.tree-item .tree-name { overflow: hidden; text-overflow: ellipsis; }
.tree-item.folder .tree-name { font-weight: 500; color: #1a2332; }

/* Code viewer panel */
.code-viewer-panel { flex: 1; min-width: 0; display: flex; flex-direction: column; background: #fff; }
.code-viewer-header { padding: 10px 16px; background: #f9fafb; border-bottom: 1px solid #e5e7eb; }
.code-viewer-filename { font-size: 13px; font-weight: 500; color: #1a2332; font-family: 'SF Mono', 'Consolas', monospace; }
.code-viewer { flex: 1; overflow: auto; }
.code-viewer pre { padding: 16px 20px; margin: 0; font-size: 13px; line-height: 1.6; background: #fff; }
.code-viewer pre code { font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace; background: transparent; }
.code-viewer-empty { display: flex; align-items: center; justify-content: center; height: 100%; color: #9ca3af; font-size: 14px; }
.code-loading { display: flex; align-items: center; justify-content: center; height: 200px; color: #9ca3af; font-size: 14px; }

/* Splash screen */
.splash { position: fixed; inset: 0; background: #fff; z-index: 1000; display: flex; align-items: center; justify-content: center; transition: opacity 0.5s; }
.splash-fade { opacity: 0; }
.splash-inner { text-align: center; }
.splash-icon { display: flex; align-items: center; justify-content: center; gap: 0; margin-bottom: 24px; }
.splash-icon .dot { width: 10px; height: 10px; background: #0f4c5c; border-radius: 50%; }
.splash-icon .line { width: 32px; height: 2px; background: #d1d5db; }
.splash-title { font-family: 'Inter', -apple-system, sans-serif; font-size: 2.5rem; font-weight: 700; color: #1a2332; letter-spacing: -0.02em; margin-bottom: 8px; }
.splash-sub { font-size: 14px; color: #9ca3af; margin-bottom: 40px; }
.splash-bar { width: 200px; height: 3px; background: #e5e7eb; border-radius: 2px; margin: 0 auto 20px; overflow: hidden; }
.splash-bar-fill { width: 0; height: 100%; background: #0f4c5c; border-radius: 2px; animation: splash-progress 1.5s ease-out forwards; }
@keyframes splash-progress { 0% { width: 0; } 100% { width: 100%; } }
.splash-loading { font-size: 13px; color: #d1d5db; }
.app-hidden { display: none; }

/* 2-column detail layout */
.detail-layout { display: flex; gap: 32px; }
.detail-main { flex: 3; min-width: 0; }
.detail-sidebar { flex: 1; min-width: 240px; max-width: 300px; }

/* Version timeline */
.sidebar-title { font-size: 11px; font-weight: 600; color: #9ca3af; letter-spacing: 0.04em; margin-bottom: 16px; }
.version-timeline { position: relative; padding-left: 28px; }
.version-line { position: absolute; left: 8px; top: 12px; bottom: 12px; width: 2px; background: #e5e7eb; }
.version-item { position: relative; margin-bottom: 24px; cursor: pointer; text-decoration: none; display: block; color: inherit; }
.version-item:last-child { margin-bottom: 0; }
.version-dot { position: absolute; left: -24px; top: 6px; width: 12px; height: 12px; border-radius: 50%; border: 2px solid #d1d5db; background: #fff; }
.version-dot.current { background: #0f4c5c; border-color: #0f4c5c; }
.version-card { padding: 16px; border: 1px solid transparent; border-radius: 8px; }
.version-card.current { background: #f9fafb; border-color: #e5e7eb; }
.version-ver { font-family: 'SF Mono', 'Consolas', monospace; font-size: 15px; font-weight: 600; color: #1a2332; }
.version-badge { display: inline-block; font-size: 10px; color: #9ca3af; border: 1px solid #d1d5db; border-radius: 100px; padding: 1px 8px; margin-left: 8px; vertical-align: middle; }
.version-badge.download { color: #6b7280; border-color: #d1d5db; cursor: pointer; }
.version-meta { font-size: 12px; color: #9ca3af; margin-top: 4px; }
.version-desc { font-size: 13px; color: #6b7280; margin-top: 6px; line-height: 1.4; }
.version-more { font-size: 13px; color: #6b7280; cursor: pointer; background: none; border: none; padding: 8px 0; margin-top: 8px; }
.version-more:hover { color: #1a2332; }

/* README section */
.readme-section { background: #fff; border: 1px solid #e5e7eb; border-radius: 10px; padding: 24px 28px; margin-top: 16px; }
.readme-content { font-size: 14px; line-height: 1.7; color: #374151; }
.readme-content h1 { font-size: 1.4em; font-weight: 700; margin: 20px 0 10px; padding-bottom: 6px; border-bottom: 1px solid #e5e7eb; }
.readme-content h2 { font-size: 1.2em; font-weight: 600; margin: 18px 0 8px; }
.readme-content h3 { font-size: 1.05em; font-weight: 600; margin: 14px 0 6px; }
.readme-content p { margin: 8px 0; }
.readme-content code { font-family: 'SF Mono','Consolas',monospace; font-size: 0.9em; background: #f3f4f6; padding: 2px 6px; border-radius: 4px; }
.readme-content pre { background: #f3f4f6; padding: 14px 18px; border-radius: 8px; overflow-x: auto; margin: 12px 0; }
.readme-content pre code { background: none; padding: 0; font-size: 13px; }
.readme-content ul, .readme-content ol { padding-left: 24px; margin: 8px 0; }
.readme-content li { margin: 4px 0; }
.readme-content a { color: #0f4c5c; }
.readme-empty { color: #9ca3af; font-size: 14px; }

/* General */
a { color: #1a2332; }
a:hover { color: #4b5563; }

/* Responsive */
@media (max-width: 768px) {
  .list-layout { flex-direction: column; }
  .filter-sidebar { width: 100%; position: static; }
  .detail-layout { flex-direction: column; }
  .detail-sidebar { max-width: 100%; }
  .files-section { flex-direction: column; min-height: auto; }
  .file-tree-panel { width: 100%; max-height: 200px; border-right: none; border-bottom: 1px solid #e5e7eb; }
}
`;
