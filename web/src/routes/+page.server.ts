import type { PageServerLoad } from './$types';
import { listPipelines, searchPipelines } from '$lib/server/pipelines.js';

export const load: PageServerLoad = async ({ url }) => {
	const q = url.searchParams.get('q') || '';
	const pipelines = q ? await searchPipelines(q) : await listPipelines();
	return { pipelines, q };
};
