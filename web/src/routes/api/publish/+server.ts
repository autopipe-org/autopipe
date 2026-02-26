import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { validateSecurity, hasErrors } from '$lib/server/security.js';
import { fetchGithubFiles } from '$lib/server/github.js';
import { db, schema } from '$lib/server/db.js';
import { eq } from 'drizzle-orm';

const { userPipelines } = schema;

// POST /api/publish — Fetch code from GitHub, validate security, store URL + metadata
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

		// 2. Fetch files from GitHub for validation
		let files;
		try {
			files = await fetchGithubFiles(github_url);
		} catch (e) {
			const message = e instanceof Error ? e.message : String(e);
			return json({ error: `Failed to fetch from GitHub: ${message}` }, { status: 400 });
		}

		// 3. Check required files
		if (!files.snakefile || !files.dockerfile) {
			return json(
				{ error: 'GitHub repository must contain Snakefile and Dockerfile' },
				{ status: 400 }
			);
		}

		// 4. Parse metadata.json
		let metadata;
		try {
			metadata = files.metadata_json ? JSON.parse(files.metadata_json) : {};
		} catch {
			return json({ error: 'metadata.json is not valid JSON' }, { status: 400 });
		}

		if (!metadata.name) {
			return json({ error: 'metadata.json must contain a "name" field' }, { status: 400 });
		}

		// 5. Security validation
		const issues = validateSecurity(files.snakefile, files.dockerfile);
		if (hasErrors(issues)) {
			return json({ error: 'Security validation failed', issues }, { status: 422 });
		}

		// 6. Upsert pipeline (by name) — store URL + metadata only
		const name = metadata.name;
		const existing = await db
			.select({ pipelineId: userPipelines.pipelineId })
			.from(userPipelines)
			.where(eq(userPipelines.name, name))
			.limit(1);

		let pipelineId: number;

		const values = {
			description: metadata.description || '',
			tools: metadata.tools || [],
			inputFormats: metadata.input_formats || [],
			outputFormats: metadata.output_formats || [],
			tags: metadata.tags || [],
			githubUrl: github_url,
			metadataJson: metadata,
			author,
			version: metadata.version || '1.0.0',
			verified: false,
			forkedFrom: typeof forked_from === 'number' ? forked_from : null
		};

		if (existing.length > 0) {
			pipelineId = existing[0].pipelineId;
			await db
				.update(userPipelines)
				.set({ ...values, updatedAt: new Date() })
				.where(eq(userPipelines.pipelineId, pipelineId));
		} else {
			const [row] = await db
				.insert(userPipelines)
				.values({ name, ...values })
				.returning({ pipelineId: userPipelines.pipelineId });
			pipelineId = row.pipelineId;
		}

		const response: Record<string, unknown> = { pipeline_id: pipelineId, author };
		if (issues.length > 0) {
			response.warnings = issues;
		}

		return json(response, { status: 200 });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};
