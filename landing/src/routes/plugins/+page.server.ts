import type { PageServerLoad } from './$types';
import { listPlugins, searchPlugins } from '$lib/server/plugins.js';

export const load: PageServerLoad = async ({ url }) => {
	const q = url.searchParams.get('q') || '';
	const plugins = q ? await searchPlugins(q) : await listPlugins();
	return { plugins, q };
};
