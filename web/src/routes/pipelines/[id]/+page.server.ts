import { error, redirect } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getPipeline, deletePipeline, getVersionChain } from '$lib/server/pipelines.js';
import { fetchGithubFiles, GithubNotFoundError } from '$lib/server/github.js';

export const load: PageServerLoad = async ({ params }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) throw error(400, 'Invalid pipeline ID');

	const pipeline = await getPipeline(id);
	if (!pipeline) throw error(404, 'Pipeline not found');

	const versionChain = await getVersionChain(id);

	// Fetch files from GitHub
	try {
		const files = await fetchGithubFiles(pipeline.github_url);
		return { pipeline, files, versionChain };
	} catch (e) {
		if (e instanceof GithubNotFoundError) {
			// GitHub link broken — auto-delete from DB
			await deletePipeline(id);
			throw redirect(302, '/?deleted=' + encodeURIComponent(pipeline.name));
		}
		throw error(502, `Failed to fetch from GitHub: ${e instanceof Error ? e.message : String(e)}`);
	}
};
