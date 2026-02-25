import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { listPlugins, searchPlugins } from '$lib/server/plugins.js';

// GET /api/plugins — list all or search with ?q=
export const GET: RequestHandler = async ({ url }) => {
	const q = url.searchParams.get('q');
	try {
		const plugins = q ? await searchPlugins(q) : await listPlugins();
		return json(plugins);
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};
