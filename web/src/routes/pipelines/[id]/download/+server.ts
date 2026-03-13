import type { RequestHandler } from './$types';
import { error } from '@sveltejs/kit';
import { getPipeline } from '$lib/server/pipelines.js';
import { fetchAllGithubFiles, GithubNotFoundError } from '$lib/server/github.js';
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

	// Fetch all files from GitHub
	let allFiles;
	try {
		allFiles = await fetchAllGithubFiles(pipeline.github_url, tag || 'main');
	} catch (e) {
		if (e instanceof GithubNotFoundError) {
			throw error(404, 'The original GitHub repository has been deleted or is no longer accessible. Please contact the pipeline author.');
		}
		throw error(502, 'Failed to fetch from GitHub');
	}

	const zip = new JSZip();
	const folder = zip.folder(pipeline.name)!;

	for (const file of allFiles) {
		folder.file(file.path, file.content);
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
