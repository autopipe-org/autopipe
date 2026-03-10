import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getPlugin, getVersionChain } from '$lib/server/plugins.js';

export const load: PageServerLoad = async ({ params }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) throw error(400, 'Invalid plugin ID');

	const plugin = await getPlugin(id);
	if (!plugin) throw error(404, 'Plugin not found');

	const versionChain = await getVersionChain(id);

	return { plugin, versionChain };
};
