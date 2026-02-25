import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { validateSecurity, hasErrors } from '$lib/server/security.js';
import { db, schema } from '$lib/server/db.js';
import { eq } from 'drizzle-orm';

const { userPipelines } = schema;

// POST /api/publish — Validate security and publish pipeline to registry
export const POST: RequestHandler = async ({ request }) => {
	try {
		const body = await request.json();
		const { pipeline, github_url, github_token } = body;

		if (!pipeline || !github_token) {
			return json({ error: 'pipeline and github_token are required' }, { status: 400 });
		}

		// 1. Validate GitHub token
		const userResp = await fetch('https://api.github.com/user', {
			headers: {
				Authorization: `Bearer ${github_token}`,
				'User-Agent': 'autopipe-registry'
			}
		});
		if (!userResp.ok) {
			return json({ error: 'Invalid GitHub token' }, { status: 401 });
		}

		// 2. Check required fields
		if (!pipeline.name || !pipeline.snakefile || !pipeline.dockerfile) {
			return json(
				{ error: 'Pipeline must have name, snakefile, and dockerfile' },
				{ status: 400 }
			);
		}

		// 3. Validate metadata_json
		if (!pipeline.metadata_json || typeof pipeline.metadata_json !== 'object') {
			return json({ error: 'metadata_json must be a valid JSON object' }, { status: 400 });
		}

		// 4. Security validation
		const issues = validateSecurity(pipeline.snakefile || '', pipeline.dockerfile || '');
		if (hasErrors(issues)) {
			return json({ error: 'Security validation failed', issues }, { status: 422 });
		}

		// 5. Upsert pipeline (by name)
		const existing = await db
			.select({ pipelineId: userPipelines.pipelineId })
			.from(userPipelines)
			.where(eq(userPipelines.name, pipeline.name))
			.limit(1);

		let pipelineId: number;

		if (existing.length > 0) {
			// Update existing
			pipelineId = existing[0].pipelineId;
			await db
				.update(userPipelines)
				.set({
					description: pipeline.description || '',
					tools: pipeline.tools || [],
					inputFormats: pipeline.input_formats || [],
					outputFormats: pipeline.output_formats || [],
					tags: pipeline.tags || [],
					snakefile: pipeline.snakefile,
					dockerfile: pipeline.dockerfile,
					configYaml: pipeline.config_yaml || '',
					metadataJson: pipeline.metadata_json,
					readme: pipeline.readme || '',
					author: pipeline.author || '',
					version: pipeline.version || '1.0.0',
					verified: pipeline.verified || false,
					updatedAt: new Date()
				})
				.where(eq(userPipelines.pipelineId, pipelineId));
		} else {
			// Insert new
			const [row] = await db
				.insert(userPipelines)
				.values({
					name: pipeline.name,
					description: pipeline.description || '',
					tools: pipeline.tools || [],
					inputFormats: pipeline.input_formats || [],
					outputFormats: pipeline.output_formats || [],
					tags: pipeline.tags || [],
					snakefile: pipeline.snakefile,
					dockerfile: pipeline.dockerfile,
					configYaml: pipeline.config_yaml || '',
					metadataJson: pipeline.metadata_json,
					readme: pipeline.readme || '',
					author: pipeline.author || '',
					version: pipeline.version || '1.0.0',
					verified: pipeline.verified || false
				})
				.returning({ pipelineId: userPipelines.pipelineId });
			pipelineId = row.pipelineId;
		}

		const response: Record<string, unknown> = { pipeline_id: pipelineId };
		if (issues.length > 0) {
			response.warnings = issues; // warnings only (errors blocked above)
		}

		return json(response, { status: 200 });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};
