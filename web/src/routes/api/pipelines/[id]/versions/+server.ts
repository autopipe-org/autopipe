import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { getVersionChain } from '$lib/server/pipelines.js';

// GET /api/pipelines/:id/versions — return the version chain for a pipeline.
//   Version chain = all rows with the same `name` AND same `author`,
//   sorted newest first by createdAt.
//   Used by the unpublish_pipeline MCP tool to discover how many versions
//   of a pipeline exist before asking the user whether to delete only the
//   latest or every version.
export const GET: RequestHandler = async ({ params }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) {
		return json({ error: 'Invalid pipeline ID' }, { status: 400 });
	}
	const chain = await getVersionChain(id);
	if (chain.length === 0) {
		return json({ error: 'Pipeline not found' }, { status: 404 });
	}
	return json({ versions: chain });
};
