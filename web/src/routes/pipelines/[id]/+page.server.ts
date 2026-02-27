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

	// Always show the latest version's code
	const latest = versionChain.length > 0 ? versionChain[0] : null;
	const codeSource =
		latest && latest.pipeline_id !== pipeline.pipeline_id
			? await getPipeline(latest.pipeline_id)
			: pipeline;
	const githubUrl = (codeSource ?? pipeline).github_url;

	// Fetch files from GitHub
	let files;
	try {
		files = await fetchGithubFiles(githubUrl);
	} catch (e) {
		if (e instanceof GithubNotFoundError) {
			await deletePipeline(id);
			throw redirect(302, '/?deleted=' + encodeURIComponent(pipeline.name));
		}
		throw error(502, `Failed to fetch from GitHub: ${e instanceof Error ? e.message : String(e)}`);
	}

	// basedOn info: when forked_from exists and the original author differs
	let basedOn: { pipeline_id: number; name: string; version: string; author: string } | null =
		null;
	if (pipeline.forked_from) {
		const parent = await getPipeline(pipeline.forked_from);
		if (parent && parent.author !== pipeline.author) {
			// Find the latest version of the original pipeline (for navigation)
			const parentChain = await getVersionChain(pipeline.forked_from);
			// Filter to same-name versions only (exclude forks with different names)
			const parentVersions = parentChain.filter(v => v.name === parent.name);
			const latestParent = parentVersions.length > 0 ? parentVersions[0] : null;
			basedOn = {
				pipeline_id: latestParent?.pipeline_id ?? parent.pipeline_id!,
				name: parent.name,
				version: parent.version,
				author: parent.author
			};
		}
	}

	return { pipeline, files, versionChain, basedOn };
};
