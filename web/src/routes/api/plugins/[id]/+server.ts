import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { getPlugin, updatePlugin, deletePlugin } from '$lib/server/plugins.js';
import {
	extractBearerToken,
	validateGithubToken,
	sanitizeErrorMessage
} from '$lib/server/security.js';

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
		return json({ error: sanitizeErrorMessage(message) }, { status: 500 });
	}
};

// PUT /api/plugins/:id (requires auth, owner only)
export const PUT: RequestHandler = async ({ params, request }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) {
		return json({ error: 'Invalid plugin ID' }, { status: 400 });
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
		const existing = await getPlugin(id);
		if (!existing) {
			return json({ error: 'Plugin not found' }, { status: 404 });
		}
		if (existing.author !== author) {
			return json({ error: 'Forbidden: you can only update your own plugins' }, { status: 403 });
		}

		const plugin = await request.json();
		plugin.author = author;
		const updated = await updatePlugin(id, plugin);
		if (!updated) {
			return json({ error: 'Plugin not found' }, { status: 404 });
		}
		return json({ updated: true });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: sanitizeErrorMessage(message) }, { status: 500 });
	}
};

// DELETE /api/plugins/:id (requires auth, owner only)
export const DELETE: RequestHandler = async ({ params, request }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) {
		return json({ error: 'Invalid plugin ID' }, { status: 400 });
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
		const existing = await getPlugin(id);
		if (!existing) {
			return json({ error: 'Plugin not found' }, { status: 404 });
		}
		if (existing.author !== author) {
			return json({ error: 'Forbidden: you can only delete your own plugins' }, { status: 403 });
		}

		const deleted = await deletePlugin(id);
		if (!deleted) {
			return json({ error: 'Plugin not found' }, { status: 404 });
		}
		return json({ deleted: true });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: sanitizeErrorMessage(message) }, { status: 500 });
	}
};
