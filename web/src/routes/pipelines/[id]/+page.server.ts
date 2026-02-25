import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getPipeline } from '$lib/server/pipelines.js';

export const load: PageServerLoad = async ({ params }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) throw error(400, 'Invalid pipeline ID');

	const pipeline = await getPipeline(id);
	if (!pipeline) throw error(404, 'Pipeline not found');

	return { pipeline };
};
