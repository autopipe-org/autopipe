import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { getPlugin, updatePlugin, deletePlugin } from '$lib/server/plugins.js';

// GET /api/plugins/:id
export const GET: RequestHandler = async ({ params }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) {
		return json({ error: 'Invalid plugin ID' }, { status: 400 });
	}
	try {
		const plugin = await getPlugin(id);
		if (!plugin) {
			return json({ error: 'Plugin not found' }, { status: 404 });
		}
		return json(plugin);
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};

// PUT /api/plugins/:id
export const PUT: RequestHandler = async ({ params, request }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) {
		return json({ error: 'Invalid plugin ID' }, { status: 400 });
	}
	try {
		const plugin = await request.json();
		const updated = await updatePlugin(id, plugin);
		if (!updated) {
			return json({ error: 'Plugin not found' }, { status: 404 });
		}
		return json({ updated: true });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};

// DELETE /api/plugins/:id
export const DELETE: RequestHandler = async ({ params }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) {
		return json({ error: 'Invalid plugin ID' }, { status: 400 });
	}
	try {
		const deleted = await deletePlugin(id);
		if (!deleted) {
			return json({ error: 'Plugin not found' }, { status: 404 });
		}
		return json({ deleted: true });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};
