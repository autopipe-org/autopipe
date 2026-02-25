import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import {
	getPipeline,
	updatePipeline,
	deletePipeline
} from '$lib/server/pipelines.js';

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
		return json({ error: message }, { status: 500 });
	}
};

// PUT /api/pipelines/:id — update a pipeline
export const PUT: RequestHandler = async ({ params, request }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) {
		return json({ error: 'Invalid pipeline ID' }, { status: 400 });
	}
	try {
		const pipeline = await request.json();
		const updated = await updatePipeline(id, pipeline);
		if (!updated) {
			return json({ error: 'Pipeline not found' }, { status: 404 });
		}
		return json({ updated: true });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};

// DELETE /api/pipelines/:id — delete a pipeline
export const DELETE: RequestHandler = async ({ params }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) {
		return json({ error: 'Invalid pipeline ID' }, { status: 400 });
	}
	try {
		const deleted = await deletePipeline(id);
		if (!deleted) {
			return json({ error: 'Pipeline not found' }, { status: 404 });
		}
		return json({ deleted: true });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};
