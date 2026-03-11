import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import {
	listPipelines,
	searchPipelines,
	insertPipeline
} from '$lib/server/pipelines.js';
import {
	extractBearerToken,
	validateGithubToken,
	sanitizeErrorMessage
} from '$lib/server/security.js';

// GET /api/pipelines — list all or search with ?q=
export const GET: RequestHandler = async ({ url }) => {
	const q = url.searchParams.get('q');
	try {
		const pipelines = q ? await searchPipelines(q) : await listPipelines();
		return json(pipelines);
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: sanitizeErrorMessage(message) }, { status: 500 });
	}
};

// POST /api/pipelines — create a new pipeline (requires GitHub auth)
export const POST: RequestHandler = async ({ request }) => {
	const token = extractBearerToken(request);
	if (!token) {
		return json({ error: 'Authorization required' }, { status: 401 });
	}
	const author = await validateGithubToken(token);
	if (!author) {
		return json({ error: 'Invalid or expired token' }, { status: 401 });
	}

	try {
		const pipeline = await request.json();

		// Validate required fields
		if (!pipeline.name || typeof pipeline.name !== 'string' || pipeline.name.trim().length === 0) {
			return json({ error: 'name is required' }, { status: 400 });
		}
		if (!pipeline.github_url || typeof pipeline.github_url !== 'string') {
			return json({ error: 'github_url is required' }, { status: 400 });
		}
		// Validate github_url points to GitHub
		if (!/^https:\/\/github\.com\/[^/]+\/[^/]+/.test(pipeline.github_url)) {
			return json({ error: 'github_url must be a valid GitHub repository URL' }, { status: 400 });
		}
		// Sanitize string length
		if (pipeline.name.length > 200) {
			return json({ error: 'name must be 200 characters or fewer' }, { status: 400 });
		}
		if (pipeline.description && pipeline.description.length > 5000) {
			return json({ error: 'description must be 5000 characters or fewer' }, { status: 400 });
		}

		pipeline.author = author; // enforce author from token
		const id = await insertPipeline(pipeline);
		return json({ pipeline_id: id }, { status: 201 });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		const status =
			message.includes('duplicate key') || message.includes('unique')
				? 409
				: 500;
		return json({ error: sanitizeErrorMessage(message) }, { status });
	}
};
