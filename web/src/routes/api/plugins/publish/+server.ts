import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { db, schema } from '$lib/server/db.js';
import { eq, sql } from 'drizzle-orm';

const { userPlugins } = schema;

// POST /api/plugins/publish — Fetch metadata from GitHub, store URL + metadata
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

		// 2. Parse GitHub URL to fetch metadata.json
		const urlMatch = github_url
			.replace(/\/$/, '')
			.match(/github\.com\/([^/]+)\/([^/]+)(?:\/tree\/[^/]+\/(.+))?/);
		if (!urlMatch) {
			return json({ error: 'Invalid GitHub URL format' }, { status: 400 });
		}
		const [, owner, repo, subpath] = urlMatch;
		const metaPath = subpath
			? `${subpath}/metadata.json`
			: 'metadata.json';

		const metaResp = await fetch(
			`https://api.github.com/repos/${owner}/${repo}/contents/${metaPath}`,
			{
				headers: {
					Accept: 'application/vnd.github.raw',
					'User-Agent': 'autopipe-registry'
				}
			}
		);
		if (!metaResp.ok) {
			return json(
				{ error: 'Cannot fetch metadata.json from GitHub repository' },
				{ status: 400 }
			);
		}

		// 3. Parse metadata
		let metadata;
		try {
			metadata = await metaResp.json();
		} catch {
			return json({ error: 'metadata.json is not valid JSON' }, { status: 400 });
		}

		if (!metadata.name) {
			return json({ error: 'metadata.json must contain a "name" field' }, { status: 400 });
		}

		// 4. Always INSERT a new record (version tracking)
		const name = metadata.name;

		// Determine forked_from: explicit param > auto-detect same name > null
		let resolvedForkedFrom: number | null = null;
		if (typeof forked_from === 'number') {
			resolvedForkedFrom = forked_from;
		} else {
			// Auto-detect: if same name exists, link to the most recent one
			const existing = await db
				.select({ pluginId: userPlugins.pluginId })
				.from(userPlugins)
				.where(eq(userPlugins.name, name))
				.orderBy(sql`${userPlugins.createdAt} DESC`)
				.limit(1);
			if (existing.length > 0) {
				resolvedForkedFrom = existing[0].pluginId;
			}
		}

		const [row] = await db
			.insert(userPlugins)
			.values({
				name,
				description: metadata.description || '',
				category: metadata.category || '',
				tags: metadata.tags || [],
				githubUrl: github_url,
				metadataJson: metadata,
				author,
				version: metadata.version || '1.0.0',
				verified: false,
				forkedFrom: resolvedForkedFrom
			})
			.returning({ pluginId: userPlugins.pluginId });

		const pluginId = row.pluginId;

		return json({ plugin_id: pluginId, name, author }, { status: 200 });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};
