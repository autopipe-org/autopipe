import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { listPlugins, searchPlugins, getPluginByName } from '$lib/server/plugins.js';
import { sanitizeErrorMessage } from '$lib/server/security.js';

// GET /api/plugins — list all, search with ?q=, or exact match with ?name=
export const GET: RequestHandler = async ({ url }) => {
	const name = url.searchParams.get('name');
	const q = url.searchParams.get('q');
	try {
		if (name) {
			const plugin = await getPluginByName(name);
			if (!plugin) return json({ error: 'Plugin not found' }, { status: 404 });
			return json(plugin);
		}
		const plugins = q ? await searchPlugins(q) : await listPlugins();
		return json(plugins);
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: sanitizeErrorMessage(message) }, { status: 500 });
	}
};
