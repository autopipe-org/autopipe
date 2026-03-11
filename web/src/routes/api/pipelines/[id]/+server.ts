import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import {
	getPipeline,
	updatePipeline,
	deletePipeline
} from '$lib/server/pipelines.js';
import {
	extractBearerToken,
	validateGithubToken,
	sanitizeErrorMessage
} from '$lib/server/security.js';

// GET /api/pipelines/:id — get pipeline details
export const GET: RequestHandler = async ({ params }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) {
		return json({ error: 'Invalid pipeline ID' }, { status: 400 });
	}
	try {
		const pipeline = await getPipeline(id);
		if (!pipeline) {
			return json({ error: 'Pipeline not found' }, { status: 404 });
		}
		return json(pipeline);
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: sanitizeErrorMessage(message) }, { status: 500 });
	}
};

// PUT /api/pipelines/:id — update a pipeline (requires auth, owner only)
export const PUT: RequestHandler = async ({ params, request }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) {
		return json({ error: 'Invalid pipeline ID' }, { status: 400 });
	}

	const token = extractBearerToken(request);
	if (!token) {
		return json({ error: 'Authorization required' }, { status: 401 });
	}
	const author = await validateGithubToken(token);
	if (!author) {
		return json({ error: 'Invalid or expired token' }, { status: 401 });
	}

	try {
		const existing = await getPipeline(id);
		if (!existing) {
			return json({ error: 'Pipeline not found' }, { status: 404 });
		}
		if (existing.author !== author) {
			return json({ error: 'Forbidden: you can only update your own pipelines' }, { status: 403 });
		}

		const pipeline = await request.json();
		pipeline.author = author;
		const updated = await updatePipeline(id, pipeline);
		if (!updated) {
			return json({ error: 'Pipeline not found' }, { status: 404 });
		}
		return json({ updated: true });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: sanitizeErrorMessage(message) }, { status: 500 });
	}
};

// DELETE /api/pipelines/:id — delete a pipeline (requires auth, owner only)
export const DELETE: RequestHandler = async ({ params, request }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) {
		return json({ error: 'Invalid pipeline ID' }, { status: 400 });
	}

	const token = extractBearerToken(request);
	if (!token) {
		return json({ error: 'Authorization required' }, { status: 401 });
	}
	const author = await validateGithubToken(token);
	if (!author) {
		return json({ error: 'Invalid or expired token' }, { status: 401 });
	}

	try {
		const existing = await getPipeline(id);
		if (!existing) {
			return json({ error: 'Pipeline not found' }, { status: 404 });
		}
		if (existing.author !== author) {
			return json({ error: 'Forbidden: you can only delete your own pipelines' }, { status: 403 });
		}

		const deleted = await deletePipeline(id);
		if (!deleted) {
			return json({ error: 'Pipeline not found' }, { status: 404 });
		}
		return json({ deleted: true });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: sanitizeErrorMessage(message) }, { status: 500 });
	}
};
