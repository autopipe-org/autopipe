import type { RequestHandler } from './$types';
import { error, json } from '@sveltejs/kit';
import { getPipeline, getVersionChain } from '$lib/server/pipelines.js';
import { fetchGithubFile, GithubNotFoundError } from '$lib/server/github.js';

export const GET: RequestHandler = async ({ params, url }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) throw error(400, 'Invalid pipeline ID');

	const filePath = url.searchParams.get('path');
	if (!filePath) throw error(400, 'Missing path parameter');

	// Validate path to prevent directory traversal
	if (filePath.includes('..') || filePath.startsWith('/')) {
		throw error(400, 'Invalid file path');
	}

	const pipeline = await getPipeline(id);
	if (!pipeline) throw error(404, 'Pipeline not found');

	const versionChain = await getVersionChain(id);
	const latest = versionChain.length > 0 ? versionChain[0] : null;
	const codeSource =
		latest && latest.pipeline_id !== pipeline.pipeline_id
			? await getPipeline(latest.pipeline_id)
			: pipeline;
	const githubUrl = (codeSource ?? pipeline).github_url;

	try {
		const content = await fetchGithubFile(githubUrl, filePath);
		return json({ content });
	} catch (e) {
		if (e instanceof GithubNotFoundError) {
			throw error(404, 'File not found');
		}
		throw error(502, 'Failed to fetch file from GitHub');
	}
};
