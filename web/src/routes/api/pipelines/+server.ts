import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import {
	listPipelines,
	searchPipelines,
	insertPipeline
} from '$lib/server/pipelines.js';

// GET /api/pipelines — list all or search with ?q=
export const GET: RequestHandler = async ({ url }) => {
	const q = url.searchParams.get('q');
	try {
		const pipelines = q ? await searchPipelines(q) : await listPipelines();
		return json(pipelines);
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};

// POST /api/pipelines — create a new pipeline
export const POST: RequestHandler = async ({ request }) => {
	try {
		const pipeline = await request.json();
		const id = await insertPipeline(pipeline);
		return json({ pipeline_id: id }, { status: 201 });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		const status =
			message.includes('duplicate key') || message.includes('unique')
				? 409
				: 500;
		return json({ error: message }, { status });
	}
};
