import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { db, schema } from '$lib/server/db.js';
import { and, eq, sql } from 'drizzle-orm';

const { userPlugins } = schema;

// POST /api/plugins/publish — Fetch manifest from GitHub, store URL + metadata
export const POST: RequestHandler = async ({ request }) => {
	try {
		const body = await request.json();
		const { github_url, github_token, forked_from } = body;

		if (!github_url || !github_token) {
			return json({ error: 'github_url and github_token are required' }, { status: 400 });
		}

		// 1. Validate GitHub token and get username for author
		const userResp = await fetch('https://api.github.com/user', {
			headers: {
				Authorization: `Bearer ${github_token}`,
				'User-Agent': 'autopipe-registry'
			}
		});
		if (!userResp.ok) {
			return json({ error: 'Invalid GitHub token' }, { status: 401 });
		}
		const githubUser = await userResp.json();
		const author = githubUser.login as string;

		// 2. Parse GitHub URL to fetch manifest
		const urlMatch = github_url
			.replace(/\/$/, '')
			.match(/github\.com\/([^/]+)\/([^/]+)(?:\/tree\/[^/]+\/(.+))?/);
		if (!urlMatch) {
			return json({ error: 'Invalid GitHub URL format' }, { status: 400 });
		}
		const [, owner, repo, subpath] = urlMatch;

		// Try manifest.json first, fall back to ro-crate-metadata.json
		let metadata: Record<string, unknown> | null = null;
		for (const filename of ['manifest.json', 'ro-crate-metadata.json']) {
			const metaPath = subpath ? `${subpath}/${filename}` : filename;
			const metaResp = await fetch(
				`https://api.github.com/repos/${owner}/${repo}/contents/${metaPath}`,
				{
					headers: {
						Accept: 'application/vnd.github.raw',
						'User-Agent': 'autopipe-registry'
					}
				}
			);
			if (metaResp.ok) {
				try {
					metadata = await metaResp.json();
					break;
				} catch {
					continue;
				}
			}
		}

		if (!metadata) {
			return json(
				{ error: 'Cannot fetch manifest.json (or ro-crate-metadata.json) from GitHub repository' },
				{ status: 400 }
			);
		}

		// 3. Fetch README.md (optional)
		let readme: string | null = null;
		for (const readmeFile of ['README.md', 'readme.md', 'README.MD']) {
			const readmePath = subpath ? `${subpath}/${readmeFile}` : readmeFile;
			const readmeResp = await fetch(
				`https://api.github.com/repos/${owner}/${repo}/contents/${readmePath}`,
				{
					headers: {
						Accept: 'application/vnd.github.raw',
						'User-Agent': 'autopipe-registry'
					}
				}
			);
			if (readmeResp.ok) {
				try {
					readme = await readmeResp.text();
					break;
				} catch {
					continue;
				}
			}
		}

		// 4. Validate required fields
		if (!metadata.name) {
			return json({ error: 'manifest.json must contain a "name" field' }, { status: 400 });
		}

		// 4. Determine INSERT vs UPDATE
		let name = metadata.name as string;
		const extensions = Array.isArray(metadata.extensions)
			? (metadata.extensions as string[])
			: [];
		const newVersion = (metadata.version as string) || '1.0.0';

		// 5. Create GitHub Release for version pinning
		let releaseTag = `v${newVersion}`;
		let releaseWarning: string | null = null;
		try {
			const releaseResp = await fetch(
				`https://api.github.com/repos/${owner}/${repo}/releases`,
				{
					method: 'POST',
					headers: {
						Authorization: `Bearer ${github_token}`,
						'Content-Type': 'application/json',
						'User-Agent': 'autopipe-registry'
					},
					body: JSON.stringify({
						tag_name: releaseTag,
						name: releaseTag,
						body: `${metadata.name} ${releaseTag}\n\nPublished via AutoPipe registry.`,
						draft: false,
						prerelease: false
					})
				}
			);
			if (!releaseResp.ok) {
				const releaseErr = await releaseResp.json().catch(() => ({}));
				if (releaseResp.status === 422 && JSON.stringify(releaseErr).includes('already_exists')) {
					// Tag already exists — reuse it
				} else {
					releaseWarning = `GitHub release creation failed (HTTP ${releaseResp.status})`;
					releaseTag = '';
				}
			}
		} catch {
			releaseWarning = 'GitHub release creation failed (network error)';
			releaseTag = '';
		}

		// Build versioned github_url for DB storage
		const baseGithubUrl = github_url.replace(/\/$/, '');
		const versionedGithubUrl = releaseTag
			? `${baseGithubUrl}/tree/${releaseTag}`
			: baseGithubUrl;

		// forked_from: trust the value the client sends (no auto-detection)
		const resolvedForkedFrom: number | null =
			typeof forked_from === 'number' ? forked_from : null;

		if (resolvedForkedFrom === null) {
			// Check for existing plugin with same name AND same author → UPDATE
			const sameAuthorMatch = await db
				.select()
				.from(userPlugins)
				.where(and(eq(userPlugins.name, name), eq(userPlugins.author, author)))
				.limit(1);

			if (sameAuthorMatch.length > 0) {
				const existing = sameAuthorMatch[0];
				const previousVersion = existing.version ?? '1.0.0';
				const existingHistory = Array.isArray(existing.versionHistory)
					? (existing.versionHistory as Array<{ version: string; updated_at: string }>)
					: [];

				// Append previous version to history (with github_url for version pinning)
				const updatedHistory = [
					...existingHistory,
					{
						version: previousVersion,
						updated_at: existing.updatedAt?.toISOString() ?? new Date().toISOString(),
						github_url: existing.githubUrl
					}
				];

				await db
					.update(userPlugins)
					.set({
						version: newVersion,
						description: (metadata.description as string) || '',
						category: (metadata.category as string) || '',
						extensions,
						tags: Array.isArray(metadata.tags) ? (metadata.tags as string[]) : [],
						githubUrl: versionedGithubUrl,
						metadataJson: metadata,
						readme: readme || '',
						versionHistory: updatedHistory,
						updatedAt: new Date()
					})
					.where(eq(userPlugins.pluginId, existing.pluginId));

				return json(
					{
						plugin_id: existing.pluginId,
						name,
						author,
						updated: true,
						previous_version: previousVersion,
						new_version: newVersion,
						...(releaseWarning && { release_warning: releaseWarning })
					},
					{ status: 200 }
				);
			}

			// Different author with same name → append suffix
			const existingName = await db
				.select({ pluginId: userPlugins.pluginId })
				.from(userPlugins)
				.where(eq(userPlugins.name, name))
				.limit(1);
			if (existingName.length > 0) {
				let suffix = 2;
				while (true) {
					const candidate = `${metadata.name} ${suffix}`;
					const dup = await db
						.select({ pluginId: userPlugins.pluginId })
						.from(userPlugins)
						.where(eq(userPlugins.name, candidate))
						.limit(1);
					if (dup.length === 0) {
						name = candidate;
						break;
					}
					suffix++;
				}
			}
		}

		// INSERT new plugin
		const [row] = await db
			.insert(userPlugins)
			.values({
				name,
				description: (metadata.description as string) || '',
				category: (metadata.category as string) || '',
				extensions,
				tags: Array.isArray(metadata.tags) ? (metadata.tags as string[]) : [],
				githubUrl: versionedGithubUrl,
				metadataJson: metadata,
				readme: readme || '',
				author,
				version: newVersion,
				verified: false,
				forkedFrom: resolvedForkedFrom
			})
			.returning({ pluginId: userPlugins.pluginId });

		const pluginId = row.pluginId;

		// Self-reference guard: should never happen, but ensure forked_from != self
		if (resolvedForkedFrom === pluginId) {
			await db
				.update(userPlugins)
				.set({ forkedFrom: null })
				.where(eq(userPlugins.pluginId, pluginId));
		}

		return json({
			plugin_id: pluginId,
			name,
			author,
			...(releaseWarning && { release_warning: releaseWarning })
		}, { status: 200 });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};
