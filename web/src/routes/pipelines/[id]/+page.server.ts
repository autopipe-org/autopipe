import { error, redirect } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getPipeline, deletePipeline, getVersionChain } from '$lib/server/pipelines.js';
import { fetchGithubTree, GithubNotFoundError } from '$lib/server/github.js';
import { env } from '$env/dynamic/public';

/**
 * Look up a pipeline by id and return a compact basedOn descriptor with the
 * latest version's pipeline_id (so the link points to the most recent version
 * in the chain).
 */
async function resolveBasedOn(parentId: number) {
	const parent = await getPipeline(parentId);
	if (!parent) return null;
	const chain = await getVersionChain(parentId);
	const versions = chain.filter(v => v.name === parent.name);
	const latest = versions.length > 0 ? versions[0] : null;
	return {
		pipeline_id: latest?.pipeline_id ?? parent.pipeline_id!,
		name: parent.name,
		version: parent.version,
		author: parent.author
	};
}

/**
 * Try to extract a Hub pipeline id from a URL. Returns the numeric id when
 * the URL points to this Hub's /pipelines/<id> path, otherwise null.
 */
function extractInternalPipelineId(url: string): number | null {
	const own = (env.PUBLIC_HUB_URL || 'https://hub.autopipe.org').replace(/\/+$/, '');
	if (!url.startsWith(own + '/pipelines/')) return null;
	const m = url.match(/\/pipelines\/(\d+)(?:\/|$|\?)/);
	if (!m) return null;
	const id = parseInt(m[1], 10);
	return Number.isNaN(id) ? null : id;
}

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

	// Fetch file tree from GitHub
	let fileTree;
	try {
		fileTree = await fetchGithubTree(githubUrl);
	} catch (e) {
		if (e instanceof GithubNotFoundError) {
			await deletePipeline(id);
			throw redirect(302, '/?deleted=' + encodeURIComponent(pipeline.name));
		}
		throw error(502, 'Failed to fetch pipeline files from GitHub');
	}

	// Resolve basedOn from forked_from when the parent author differs.
	// If the parent pipeline has been unpublished from the Hub, track it so the
	// template can render "original pipeline has been deleted" instead of a
	// broken link. We never cascade-delete forks, so the dangling reference is
	// expected and handled here.
	let basedOn: { pipeline_id: number; name: string; version: string; author: string } | null =
		null;
	let forkedFromDeleted: { id: number } | null = null;
	if (pipeline.forked_from) {
		const parent = await getPipeline(pipeline.forked_from);
		if (parent && parent.author !== pipeline.author) {
			basedOn = await resolveBasedOn(pipeline.forked_from);
		} else if (!parent) {
			forkedFromDeleted = { id: pipeline.forked_from };
		}
	}

	// If basedOn is still null but based_on_url points to our own Hub, treat
	// it as an internal lineage and link to the parent pipeline page. We do
	// this regardless of author so users see the lineage they recorded via
	// download_pipeline → publish_pipeline.
	let basedOnUrl: string | null = pipeline.based_on_url ?? null;
	if (!basedOn && !forkedFromDeleted && basedOnUrl) {
		const internalId = extractInternalPipelineId(basedOnUrl);
		if (internalId !== null && internalId !== pipeline.pipeline_id) {
			const internal = await resolveBasedOn(internalId);
			if (internal) {
				basedOn = internal;
				// Internal lineage is now represented by `basedOn` — clear the raw
				// URL so the template doesn't render a duplicate "External" link.
				basedOnUrl = null;
			} else {
				// The Hub URL points to a now-deleted pipeline.
				forkedFromDeleted = { id: internalId };
				basedOnUrl = null;
			}
		}
	}

	return { pipeline, fileTree, githubUrl, versionChain, basedOn, basedOnUrl, forkedFromDeleted };
};
