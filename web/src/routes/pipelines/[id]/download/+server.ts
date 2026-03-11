import type { RequestHandler } from './$types';
import { error } from '@sveltejs/kit';
import { getPipeline } from '$lib/server/pipelines.js';
import { fetchGithubFiles, fetchGithubFilesAtRef, GithubNotFoundError } from '$lib/server/github.js';
import JSZip from 'jszip';

export const GET: RequestHandler = async ({ params, url }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) throw error(400, 'Invalid pipeline ID');

	const pipeline = await getPipeline(id);
	if (!pipeline) throw error(404, 'Pipeline not found');

	const tag = url.searchParams.get('tag'); // e.g., "pipeline-name/v1.0.0"

	// Validate tag format to prevent unexpected git ref access
	if (tag && !/^[a-zA-Z0-9._\/ -]+$/.test(tag)) {
		throw error(400, 'Invalid tag format');
	}

	// Fetch files from GitHub (at specific tag if provided)
	let files;
	try {
		files = tag
			? await fetchGithubFilesAtRef(pipeline.github_url, tag)
			: await fetchGithubFiles(pipeline.github_url);
	} catch (e) {
		if (e instanceof GithubNotFoundError) {
			throw error(404, 'The original GitHub repository has been deleted or is no longer accessible. Please contact the pipeline author.');
		}
		throw error(502, 'Failed to fetch from GitHub');
	}

	const zip = new JSZip();
	const folder = zip.folder(pipeline.name)!;

	const entries: [string, string][] = [
		['Snakefile', files.snakefile],
		['Dockerfile', files.dockerfile],
		['config.yaml', files.config_yaml],
		['ro-crate-metadata.json', files.metadata_json],
		['README.md', files.readme]
	];

	for (const [name, content] of entries) {
		if (content) {
			folder.file(name, content);
		}
	}

	const buf = await zip.generateAsync({ type: 'arraybuffer', compression: 'DEFLATE' });

	// Sanitize name for use in Content-Disposition header
	const safeName = (pipeline.name || 'pipeline').replace(/[^a-zA-Z0-9._-]/g, '_');
	const safeVersion = (tag?.split('/').pop() || pipeline.version || 'latest').replace(/[^a-zA-Z0-9._-]/g, '_');
	const zipName = tag
		? `${safeName}-${safeVersion}.zip`
		: `${safeName}.zip`;

	return new Response(buf, {
		headers: {
			'Content-Type': 'application/zip',
			'Content-Disposition': `attachment; filename="${zipName}"`
		}
	});
};
