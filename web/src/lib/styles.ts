export const CSS = `
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Inter', sans-serif; background: #fafafa; color: #111; line-height: 1.5; }

/* Header */
header { padding: 14px 40px; border-bottom: 1px solid #eee; background: #fff; }
.logo { font-size: 1.15rem; font-weight: 700; color: #111; text-decoration: none; letter-spacing: -0.02em; }

/* Main layout - wide */
main { max-width: 1200px; margin: 0 auto; padding: 32px 40px; }

/* Sections */
.section { margin-bottom: 28px; }
.section-title { font-size: 15px; font-weight: 600; color: #333; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 2px solid #e5e5e5; }
.section-count { font-weight: 400; color: #999; font-size: 13px; }

/* Search */
.search input { width: 100%; padding: 11px 16px; border: 1px solid #ddd; border-radius: 8px; font-size: 14px; background: #fff; transition: border-color 0.2s; outline: none; }
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
.card-title { font-size: 15px; font-weight: 600; color: #111; margin-bottom: 12px; }
.card-version { font-weight: 400; color: #aaa; font-size: 13px; margin-left: 8px; }
.card-desc { color: #666; font-size: 13px; margin-bottom: 14px; line-height: 1.5; }
.card-tags { display: flex; flex-wrap: wrap; gap: 6px; }
.tag { display: inline-block; padding: 3px 10px; border-radius: 100px; font-size: 11px; background: #f0f0f0; color: #666; font-weight: 500; }
.tag.tool { background: #111; color: #fff; }
.empty { text-align: center; color: #999; padding: 60px 20px; font-size: 14px; background: #fff; }

/* Detail page */
.back-link { display: inline-block; font-size: 13px; color: #888; text-decoration: none; margin-bottom: 24px; }
.back-link:hover { color: #111; }
.detail-header { display: flex; justify-content: space-between; align-items: flex-start; gap: 24px; margin-bottom: 24px; }
.detail-header h2 { font-size: 1.5rem; font-weight: 700; letter-spacing: -0.02em; margin-bottom: 8px; }
.detail-desc { color: #666; font-size: 14px; }
.detail-info { display: flex; gap: 48px; padding: 20px 0; border-top: 1px solid #eee; border-bottom: 1px solid #eee; margin-bottom: 16px; }
.detail-info-item { display: flex; flex-direction: column; gap: 4px; }
.label { font-size: 11px; font-weight: 600; color: #999; letter-spacing: 0.04em; }
.value { font-size: 14px; color: #111; }
.detail-tags { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; padding: 16px 0; margin-bottom: 24px; }
.btn { display: inline-block; padding: 9px 22px; background: #111; color: #fff; text-decoration: none; border-radius: 8px; font-size: 13px; font-weight: 500; transition: background 0.2s; white-space: nowrap; }
.btn:hover { background: #333; }

/* File tabs */
.files-section { background: #fff; border: 1px solid #e5e5e5; border-radius: 10px; overflow: hidden; }
.tab-bar { display: flex; border-bottom: 1px solid #e5e5e5; background: #fafafa; overflow-x: auto; }
.tab-btn { padding: 10px 20px; border: none; background: none; font-size: 13px; font-weight: 500; color: #888; cursor: pointer; border-bottom: 2px solid transparent; transition: color 0.15s, border-color 0.15s; white-space: nowrap; }
.tab-btn:hover { color: #111; }
.tab-btn.active { color: #111; border-bottom-color: #111; }
.tab-panel { display: none; }
.tab-panel.active { display: block; }
.tab-panel pre { padding: 24px; overflow-x: auto; font-size: 13px; line-height: 1.6; background: #fff; margin: 0; }

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

/* General */
a { color: #111; }
a:hover { color: #555; }
`;
